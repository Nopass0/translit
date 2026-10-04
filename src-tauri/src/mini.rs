//! Downloadable CPU-only translation model, bounded workers, and owned server.
use serde::{Deserialize, Serialize};
use std::{
    fs,
    net::TcpListener,
    os::windows::{io::AsRawHandle, process::CommandExt},
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::{Arc, Mutex},
};
use tauri::{AppHandle, Emitter, Manager};
use windows_sys::Win32::{Foundation::CloseHandle, System::JobObjects::*};
const MODEL_URL:&str="https://huggingface.co/ordois/opus-mt-en-ru-ctranslate2-int8/resolve/9ae164a637a7e380d3e09768899c886af9925afe/opus-mt-en-ru-ctranslate2-int8.zip";
const MODEL_HASH: &str = "f4c3ab9becb25549673ae407c5db73f86bcf57719426e497c592415a5b512bcd";
const ENGINE_URL: &str = "https://www.python.org/ftp/python/3.12.10/python-3.12.10-embed-amd64.zip";
const ENGINE_HASH: &str = "4acbed6dd1c744b0376e3b1cf57ce906f9dc9e95e68824584c8099a63025a3c3";
#[derive(Deserialize)]
struct Dependency {
    url: String,
    hash: String,
    file: String,
    destination: String,
    label: String,
}
#[derive(Serialize)]
pub struct MiniStatus {
    pub installed: bool,
    pub running: bool,
    pub ready: bool,
    pub parser_only: bool,
    pub model: String,
    pub size_mb: u32,
    pub threads: u32,
    pub device: String,
    pub grammar: bool,
    pub installing: bool,
}
pub struct Server {
    pub child: Child,
    pub job: usize,
    pub endpoint: String,
    pub token: String,
    pub configuration: String,
    pub ready: bool,
}
impl Drop for Server {
    /// Closing the owned job terminates the inference process, also on app failure.
    fn drop(&mut self) {
        unsafe {
            CloseHandle(self.job as _);
        }
        let _ = self.child.wait();
    }
}
#[derive(Clone)]
pub struct Mini {
    pub root: PathBuf,
    pub server: Arc<Mutex<Option<Server>>>,
    pub installing: Arc<std::sync::atomic::AtomicBool>,
}
impl Mini {
    /// Creates an unloaded model manager; no download or inference starts here.
    pub fn new(root: PathBuf) -> Self {
        Self {
            root,
            server: Arc::new(Mutex::new(None)),
            installing: Arc::new(std::sync::atomic::AtomicBool::new(false)),
        }
    }
    /// Reports local installation and live child status without starting a model.
    pub fn status(&self) -> MiniStatus {
        let mut server = self.server.lock().unwrap();
        if server
            .as_mut()
            .is_some_and(|s| matches!(s.child.try_wait(), Ok(Some(_))))
        {
            *server = None;
        }
        MiniStatus {
            installed: self
                .root
                .join("model/model.bin")
                .metadata()
                .is_ok_and(|m| m.len() == 78276475)
                && self.root.join("installed.json").exists()
                && self.root.join("engine/python.exe").exists()
                && self.root.join("grammar-ready.json").exists(),
            running: server.is_some(),
            ready: server.as_ref().is_some_and(|s| s.ready),
            parser_only: server
                .as_ref()
                .is_some_and(|s| s.configuration.ends_with(":grammar")),
            model: "OPUS-MT EN → RU · int8".into(),
            size_mb: 75,
            threads: 2,
            device: "cpu".into(),
            grammar: false,
            installing: self.installing.load(std::sync::atomic::Ordering::SeqCst),
        }
    }
    /// Stops only the inference child owned by this application instance.
    pub fn stop(&self) {
        self.server.lock().unwrap().take();
    }
    /// Starts a private localhost server in a kill-on-close job, with two CPU threads.
    pub fn start(&self, threads: u32) -> Result<(String, String), String> {
        self.start_worker(threads, false)
    }
    /// Starts only the small parser, or reuses a loaded translation worker.
    pub fn start_grammar(&self, threads: u32) -> Result<(String, String), String> {
        self.start_worker(threads, true)
    }
    /// Creates one private embedded process with the requested model loading policy.
    fn start_worker(&self, threads: u32, grammar_only: bool) -> Result<(String, String), String> {
        let signature = format!(
            "{threads}:{}",
            if grammar_only {
                "grammar"
            } else {
                "translation"
            }
        );
        let mut server = self.server.lock().unwrap();
        if let Some(s) = server.as_mut() {
            if s.child.try_wait().map_err(|e| e.to_string())?.is_none()
                && (s.configuration == signature
                    || (grammar_only && s.configuration == format!("{threads}:translation")))
            {
                return Ok((s.endpoint.clone(), s.token.clone()));
            }
            *server = None;
        }
        let executable = self.root.join("engine/python.exe");
        let model = self.root.join("model");
        if !executable.exists()
            || !model.join("model.bin").exists()
            || !self.root.join("installed.json").exists()
        {
            return Err("Сначала установите мини-модель в настройках Translit".into());
        }
        let port = TcpListener::bind("127.0.0.1:0")
            .map_err(|e| e.to_string())?
            .local_addr()
            .map_err(|e| e.to_string())?
            .port();
        let mut token_bytes = [0u8; 32];
        getrandom::fill(&mut token_bytes).map_err(|e| e.to_string())?;
        let token = token_bytes
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect::<String>();
        let log = fs::File::create(self.root.join("mini-server.log")).map_err(|e| e.to_string())?;
        fs::write(
            self.root.join("engine/mini_server.py"),
            include_str!("mini_server.py"),
        )
        .map_err(|e| e.to_string())?;
        fs::write(
            self.root.join("engine/grammar_worker.py"),
            include_str!("grammar_worker.py"),
        )
        .map_err(|e| e.to_string())?;
        let mut child = Command::new(executable)
            .args(["-I", "-X", "utf8"])
            .arg(self.root.join("engine/mini_server.py"))
            .arg("--root")
            .arg(model)
            .args(["--port", &port.to_string()])
            .args(["--threads", &threads.to_string()])
            .args(if grammar_only {
                vec!["--nlp-only"]
            } else {
                vec![]
            })
            .env("TRANSLIT_WORKER_TOKEN", &token)
            .stdout(Stdio::from(log.try_clone().map_err(|e| e.to_string())?))
            .stderr(Stdio::from(log))
            .creation_flags(0x08004000)
            .spawn()
            .map_err(|e| e.to_string())?;
        // SAFETY: job information and the process handle belong to this manager.
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
                return Err("Не удалось создать контролируемый процесс модели".into());
            }
            job as usize
        };
        let endpoint = format!("http://127.0.0.1:{port}");
        *server = Some(Server {
            child,
            job,
            endpoint: endpoint.clone(),
            token: token.clone(),
            configuration: signature,
            ready: false,
        });
        Ok((endpoint, token))
    }
    /// Downloads pinned model and engine with SHA-256 verification and progress.
    pub async fn install(&self, app: &AppHandle) -> Result<(), String> {
        use std::sync::atomic::Ordering;
        if self.installing.swap(true, Ordering::SeqCst) {
            return Err("Установка уже выполняется".into());
        }
        let result=async{
            fs::create_dir_all(&self.root).map_err(|e|e.to_string())?;
            let bundled=app.path().resource_dir().map_err(|e|e.to_string())?.join("models/opus");
            if bundled.join("grammar-ready.json").exists() && bundled.join("model/model.bin").metadata().is_ok_and(|m|m.len()==78276475) {
                let root=self.root.clone();
                tauri::async_runtime::spawn_blocking(move||{
                    let _=fs::remove_file(root.join("grammar-ready.json"));
                    copy_tree(&bundled,&root)?;
                    fs::copy(bundled.join("installed.json"),root.join("installed.json")).map_err(|e|e.to_string())?;
                    fs::copy(bundled.join("grammar-ready.json"),root.join("grammar-ready.json")).map_err(|e|e.to_string())?;
                    Ok::<(),String>(())
                }).await.map_err(|e|e.to_string())??;
                return Ok(());
            }
            download(app,ENGINE_URL,ENGINE_HASH,&self.root.join("engine.zip"),"Автономный CPU runtime").await?;
            let root=self.root.clone();tauri::async_runtime::spawn_blocking(move||extract(&root,"engine.zip","engine",false)).await.map_err(|e|e.to_string())??;
            let dependencies:Vec<Dependency> = serde_json::from_str(include_str!("mini-dependencies.json").trim_start_matches('\u{feff}')).map_err(|e|e.to_string())?;
            let grammar:Vec<Dependency> = serde_json::from_str(include_str!("grammar-dependencies.json")).map_err(|e|e.to_string())?;
            for dependency in dependencies.into_iter().chain(grammar) {
                download(app,&dependency.url,&dependency.hash,&self.root.join(&dependency.file),&dependency.label).await?;
                let root=self.root.clone();tauri::async_runtime::spawn_blocking(move||extract(&root,&dependency.file,&dependency.destination,false)).await.map_err(|e|e.to_string())??;
            }
            download(app,MODEL_URL,MODEL_HASH,&self.root.join("model.zip"),"OPUS-MT EN → RU · 75 MiB").await?;
            let root=self.root.clone();tauri::async_runtime::spawn_blocking(move||extract(&root,"model.zip","model",true)).await.map_err(|e|e.to_string())??;
            fs::write(self.root.join("engine/python312._pth"),"python312.zip\n.\nLib/site-packages\nimport site\n").map_err(|e|e.to_string())?;
            fs::write(self.root.join("engine/mini_server.py"),include_str!("mini_server.py")).map_err(|e|e.to_string())?;
            fs::write(self.root.join("engine/grammar_worker.py"),include_str!("grammar_worker.py")).map_err(|e|e.to_string())?;
            fs::write(self.root.join("grammar-ready.json"),"{\"spacy\":\"3.8.11\",\"model\":\"en_core_web_sm-3.8.0\"}").map_err(|e|e.to_string())?;
            fs::write(self.root.join("SOURCES.txt"),"Helsinki-NLP/opus-mt-en-ru (Apache 2.0), int8 conversion ordois revision 9ae164a637a7e380d3e09768899c886af9925afe: https://huggingface.co/ordois/opus-mt-en-ru-ctranslate2-int8\nCTranslate2 4.8.0 (MIT): https://github.com/OpenNMT/CTranslate2\nPython 3.12.10 (PSF), NumPy 2.2.6 (BSD), SentencePiece 0.2.0 (Apache 2.0), PyYAML 6.0.2 (MIT), setuptools 80.9.0 (MIT). Licenses remain in runtime and *.dist-info directories.\n").map_err(|e|e.to_string())?;
            fs::write(self.root.join("installed.json"),serde_json::json!({"model_sha256":MODEL_HASH,"runtime_sha256":ENGINE_HASH}).to_string()).map_err(|e|e.to_string())?;
            Ok(())
        }.await;
        self.installing.store(false, Ordering::SeqCst);
        result
    }
}
/// Copies bundled autonomous files to the writable model directory without needing Python or pip.
pub(crate) fn copy_tree(source: &Path, destination: &Path) -> Result<(), String> {
    fs::create_dir_all(destination).map_err(|e| e.to_string())?;
    for entry in fs::read_dir(source).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        let kind = entry.file_type().map_err(|e| e.to_string())?;
        let target = destination.join(entry.file_name());
        if kind.is_dir() {
            copy_tree(&entry.path(), &target)?;
        } else if kind.is_file()
            && entry.file_name() != "installed.json"
            && entry.file_name() != "grammar-ready.json"
        {
            fs::copy(entry.path(), target).map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}
/// Streams a verified download into a temporary file, then commits it atomically.
pub(crate) async fn download(
    app: &AppHandle,
    url: &str,
    expected: &str,
    path: &Path,
    label: &str,
) -> Result<(), String> {
    crate::downloads::download(url, expected, path, |received, total| {
        let _ = app.emit(
            "model-progress",
            serde_json::json!({"label":label,"received":received,"total":total}),
        );
    })
    .await
}
/// Extracts only enclosed engine paths, preventing archive traversal outside the model directory.
pub(crate) fn extract(
    root: &Path,
    archive_name: &str,
    destination: &str,
    strip_directory: bool,
) -> Result<(), String> {
    let mut archive =
        zip::ZipArchive::new(fs::File::open(root.join(archive_name)).map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())?;
    let directory = root.join(destination);
    fs::create_dir_all(&directory).map_err(|e| e.to_string())?;
    for i in 0..archive.len() {
        let mut entry = archive.by_index(i).map_err(|e| e.to_string())?;
        let relative = entry
            .enclosed_name()
            .ok_or("Некорректный путь в архиве движка")?;
        if entry.is_dir() {
            continue;
        }
        let relative = if strip_directory {
            relative.components().skip(1).collect::<PathBuf>()
        } else {
            relative.to_owned()
        };
        if relative.as_os_str().is_empty() {
            continue;
        }
        let destination = directory.join(relative);
        if let Some(parent) = destination.parent() {
            fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        let mut file = fs::File::create(destination).map_err(|e| e.to_string())?;
        std::io::copy(&mut entry, &mut file).map_err(|e| e.to_string())?;
    }
    Ok(())
}
