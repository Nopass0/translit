//! A resident, authenticated Laya decision worker independent of translation models.
use crate::{
    mini::{self, Server},
    runtime::Runtime,
};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    net::TcpListener,
    os::windows::{io::AsRawHandle, process::CommandExt},
    path::PathBuf,
    process::{Command, Stdio},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    time::Duration,
};
use tauri::{AppHandle, Emitter};
use windows_sys::Win32::{Foundation::CloseHandle, System::JobObjects::*};

#[derive(Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct DecisionSettings {
    pub enabled: bool,
    pub preload: bool,
    pub auto_install: bool,
    pub threads: u32,
    pub tokens: u32,
}
impl Default for DecisionSettings {
    /// Enables automatic installation and warm startup with a small token budget.
    fn default() -> Self {
        Self {
            enabled: true,
            preload: true,
            auto_install: true,
            threads: 2,
            tokens: 512,
        }
    }
}
#[derive(Clone, Serialize, Deserialize)]
pub struct Answer {
    pub choice: String,
    pub confidence: f64,
    pub probabilities: Vec<f64>,
    pub truncated: bool,
}
#[derive(Clone, Serialize, Deserialize)]
pub struct Analysis {
    pub answers: std::collections::BTreeMap<String, Answer>,
    pub elapsed_ms: u64,
}
/// Binds model scores back to dictionary glosses without treating confidence as truth.
#[derive(Clone, Serialize, Deserialize)]
pub struct Evidence {
    pub candidates: Vec<String>,
    pub analysis: Analysis,
}
#[derive(Serialize)]
pub struct Status {
    pub installed: bool,
    pub running: bool,
    pub ready: bool,
    pub installing: bool,
    pub error: String,
}
#[derive(Deserialize)]
struct Asset {
    file: String,
    url: String,
    hash: String,
    destination: String,
}
#[derive(Clone)]
pub struct Decision {
    pub root: PathBuf,
    pub server: Arc<Mutex<Option<Server>>>,
    pub installing: Arc<AtomicBool>,
    pub error: Arc<Mutex<String>>,
    preparation: Arc<tokio::sync::Mutex<()>>,
}
impl Decision {
    /// Creates an unloaded manager in application data.
    pub fn new(root: PathBuf) -> Self {
        Self {
            root,
            server: Arc::new(Mutex::new(None)),
            installing: Arc::new(AtomicBool::new(false)),
            error: Arc::new(Mutex::new(String::new())),
            preparation: Arc::new(tokio::sync::Mutex::new(())),
        }
    }
    /// Reads the pinned model and wheel manifest embedded in the executable.
    fn assets() -> Vec<Asset> {
        serde_json::from_str(include_str!("decision-assets.json")).expect("Invalid Laya manifest")
    }
    /// Checks that the complete verified installation marker and model files exist.
    pub fn installed(&self) -> bool {
        self.root.join("installed.json").exists()
            && Self::assets().iter().all(|a| {
                if a.destination == "model" {
                    self.root.join("model").join(&a.file).is_file()
                } else {
                    self.root.join(&a.file).is_file()
                }
            })
    }
    /// Returns live worker state, clearing children that exited during loading.
    pub fn status(&self) -> Status {
        let mut server = self.server.lock().unwrap();
        if server
            .as_mut()
            .is_some_and(|s| matches!(s.child.try_wait(), Ok(Some(_))))
        {
            *server = None;
            *self.error.lock().unwrap() =
                "Процесс Laya завершился. Повторите запуск или уменьшите потоки.".into();
        }
        Status {
            installed: self.installed(),
            running: server.is_some(),
            ready: server.as_ref().is_some_and(|s| s.ready),
            installing: self.installing.load(Ordering::SeqCst),
            error: self.error.lock().unwrap().clone(),
        }
    }
    /// Frees only the process owned by this manager.
    pub fn stop(&self) {
        self.server.lock().unwrap().take();
    }
    /// Resumes downloads, verifies every file, then installs wheels in a private search path.
    pub async fn install(&self, app: &AppHandle) -> Result<(), String> {
        if self.installing.swap(true, Ordering::SeqCst) {
            return Err("Laya уже устанавливается".into());
        }
        let result: Result<(), String> = async {
            fs::create_dir_all(&self.root).map_err(|e|e.to_string())?;
            for asset in Self::assets() {
                let path = if asset.destination == "model" { self.root.join("model").join(&asset.file) } else { self.root.join(&asset.file) };
                if let Some(parent) = path.parent() { fs::create_dir_all(parent).map_err(|e|e.to_string())?; }
                mini::download(app, &asset.url, &asset.hash, &path, "Laya Multilingual").await?;
                if asset.destination == "engine" { let root=self.root.clone(); tauri::async_runtime::spawn_blocking(move || mini::extract(&root,&asset.file,"engine",false)).await.map_err(|e|e.to_string())??; }
            }
            fs::write(self.root.join("installed.json"), include_str!("decision-assets.json")).map_err(|e|e.to_string())?;
            fs::write(self.root.join("SOURCES.txt"), "Laya Multilingual by Convai Innovations, Apache-2.0\nhttps://huggingface.co/convaiinnovations/laya-multilingual\nONNX export: https://huggingface.co/onnx-community/laya-multilingual-ONNX\nONNX Runtime MIT: https://github.com/microsoft/onnxruntime\nTokenizers Apache-2.0: https://github.com/huggingface/tokenizers\n").map_err(|e|e.to_string())?;
            Ok(())
        }.await;
        self.installing.store(false, Ordering::SeqCst);
        result
    }
    /// Starts the pinned Python worker in a kill-on-close job, reusing matching live instances.
    fn start(&self, python: PathBuf, threads: u32) -> Result<(String, String), String> {
        let threads = threads.clamp(1, 8);
        let mut server = self.server.lock().unwrap();
        if let Some(s) = server.as_mut() {
            if s.configuration == threads.to_string()
                && s.child.try_wait().map_err(|e| e.to_string())?.is_none()
            {
                return Ok((s.endpoint.clone(), s.token.clone()));
            }
            *server = None;
        }
        if !self.installed() {
            return Err("Laya ещё не установлена".into());
        }
        let port = TcpListener::bind("127.0.0.1:0")
            .map_err(|e| e.to_string())?
            .local_addr()
            .map_err(|e| e.to_string())?
            .port();
        let mut bytes = [0u8; 32];
        getrandom::fill(&mut bytes).map_err(|e| e.to_string())?;
        let token = bytes.iter().map(|b| format!("{b:02x}")).collect::<String>();
        let script = self.root.join("decision_worker.py");
        fs::write(&script, include_str!("decision_worker.py")).map_err(|e| e.to_string())?;
        let log = fs::File::create(self.root.join("worker.log")).map_err(|e| e.to_string())?;
        let mut child = Command::new(python)
            .args(["-I", "-X", "utf8"])
            .arg(&script)
            .arg("--root")
            .arg(&self.root)
            .args([
                "--port",
                &port.to_string(),
                "--threads",
                &threads.to_string(),
            ])
            .env("TRANSLIT_WORKER_TOKEN", &token)
            .stdout(Stdio::from(log.try_clone().map_err(|e| e.to_string())?))
            .stderr(Stdio::from(log))
            .creation_flags(0x08000000)
            .spawn()
            .map_err(|e| e.to_string())?;
        // SAFETY: all job handles and the child process belong to this manager.
        let job = unsafe {
            let job = CreateJobObjectW(std::ptr::null(), std::ptr::null());
            let mut info: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = std::mem::zeroed();
            info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
            if job.is_null()
                || SetInformationJobObject(
                    job,
                    JobObjectExtendedLimitInformation,
                    &info as *const _ as _,
                    std::mem::size_of_val(&info) as u32,
                ) == 0
                || AssignProcessToJobObject(job, child.as_raw_handle() as _) == 0
            {
                if !job.is_null() {
                    CloseHandle(job);
                }
                let _ = child.kill();
                return Err("Не удалось создать процесс Laya".into());
            }
            job as usize
        };
        let endpoint = format!("http://127.0.0.1:{port}");
        *server = Some(Server {
            child,
            job,
            endpoint: endpoint.clone(),
            token: token.clone(),
            configuration: threads.to_string(),
            ready: false,
        });
        Ok((endpoint, token))
    }
    /// Installs if permitted and preloads independently of the selected translator.
    pub async fn prepare(&self, rt: &Runtime, app: &AppHandle, force: bool) -> Result<(), String> {
        let _guard = self.preparation.lock().await;
        let settings = rt.inner.lock().unwrap().data.settings.decision.clone();
        if !force && (!settings.enabled || !settings.preload) {
            return Ok(());
        }
        *self.error.lock().unwrap() = String::new();
        let result: Result<(), String> = async {
            if !rt.mini.status().installed {
                rt.mini.install(app).await?;
            }
            if !self.installed() {
                if !settings.auto_install && !force {
                    return Ok(());
                }
                self.install(app).await?;
            }
            if rt.inner.lock().unwrap().data.settings.decision != settings {
                return Ok(());
            }
            let manager = self.clone();
            let python = rt.mini.root.join("engine/python.exe");
            let threads = settings.threads;
            let (endpoint, token) =
                tauri::async_runtime::spawn_blocking(move || manager.start(python, threads))
                    .await
                    .map_err(|e| e.to_string())??;
            let client = reqwest::Client::builder()
                .timeout(Duration::from_secs(1))
                .build()
                .map_err(|e| e.to_string())?;
            let deadline = std::time::Instant::now() + Duration::from_secs(90);
            while std::time::Instant::now() < deadline {
                if !self.status().running {
                    return Err("Laya не запустилась. Откройте журнал worker.log.".into());
                }
                if client
                    .get(format!("{endpoint}/health"))
                    .bearer_auth(&token)
                    .send()
                    .await
                    .is_ok_and(|r| r.status().is_success())
                {
                    let mut worker = self.server.lock().unwrap();
                    if let Some(s) = worker.as_mut().filter(|s| s.token == token) {
                        s.ready = true;
                        let _ = app.emit("decision-ready", ());
                        return Ok(());
                    }
                    return Err("Настройки Laya изменились".into());
                }
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
            Err("Истекло время загрузки Laya".into())
        }
        .await;
        if let Err(e) = &result {
            *self.error.lock().unwrap() = e.clone();
        }
        result
    }
    /// Uses only already warm weights; missing or busy workers never hold up translation startup.
    pub async fn analyze(
        &self,
        state: String,
        candidates: Vec<String>,
        tokens: u32,
    ) -> Result<Analysis, String> {
        let (endpoint, token) = {
            let worker = self.server.lock().unwrap();
            let s = worker
                .as_ref()
                .filter(|s| s.ready)
                .ok_or("Laya ещё загружается")?;
            (s.endpoint.clone(), s.token.clone())
        };
        let mut questions = serde_json::json!({"intent":{"question":"What is the speaker doing in the current dialogue?","choices":["asking for information","requesting an action","giving information","warning or threatening","joking or teasing","expressing emotion"]},"usage":{"question":"Is the selected expression used literally or figuratively here?","choices":["literal meaning","figurative or idiomatic meaning"]},"tone":{"question":"What tone does the current speaker use?","choices":["neutral","friendly or warm","sarcastic or teasing","hostile or threatening","formal or distant"]}});
        if candidates.len() >= 2 {
            questions["sense"] = serde_json::json!({"question":"Which Russian dictionary meaning best matches the selected English word in the current sentence?","choices":candidates});
        }
        reqwest::Client::builder()
            .timeout(Duration::from_secs(8))
            .build()
            .map_err(|e| e.to_string())?
            .post(format!("{endpoint}/decide"))
            .bearer_auth(token)
            .json(&serde_json::json!({"state":state,"questions":questions,"tokens":tokens}))
            .send()
            .await
            .map_err(|e| e.to_string())?
            .error_for_status()
            .map_err(|e| e.to_string())?
            .json()
            .await
            .map_err(|e| e.to_string())
    }
}
