//! Pinned optional grammar models with CPU/Vulkan acceleration and owned lifecycle.
use crate::{
    mini::{self, Server},
    models::TranslatorSettings,
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
};
use tauri::{AppHandle, Manager};
use windows_sys::Win32::{Foundation::CloseHandle, System::JobObjects::*};

#[derive(Clone, Serialize, Deserialize)]
pub struct Model {
    pub id: String,
    pub name: String,
    pub size: u64,
    pub grammar: bool,
    pub gpu: bool,
    pub url: String,
    pub hash: String,
    pub source: String,
}
/// Returns pinned model metadata; catalog updates never silently replace installed weights.
pub fn catalog() -> Vec<Model> {
    serde_json::from_str(include_str!("model-catalog.json"))
        .expect("Invalid built-in model catalog")
}
/// Chooses a bounded automatic CPU count, leaving logical processors for the game.
pub fn threads(settings: &TranslatorSettings) -> u32 {
    let available = std::thread::available_parallelism()
        .map(|n| n.get() as u32)
        .unwrap_or(2);
    if settings.threads == 0 {
        (available / 2).clamp(2, 8).min(available)
    } else {
        settings.threads.clamp(1, 64).min(available)
    }
}
#[derive(Clone)]
pub struct Llm {
    pub root: PathBuf,
    pub server: Arc<Mutex<Option<Server>>>,
    pub installing: Arc<AtomicBool>,
}
impl Llm {
    /// Finds a Vulkan adapter, repairing missing NVIDIA ICD discovery only for this child.
    fn vulkan_manifest(&self, settings: &TranslatorSettings) -> Result<Option<PathBuf>, String> {
        let engine = self.engine(settings).join("llama-server.exe");
        let probe = |manifest: Option<&PathBuf>| -> bool {
            let mut command = Command::new(&engine);
            command.arg("--list-devices").creation_flags(0x08000000);
            if let Some(path) = manifest {
                command.env("VK_DRIVER_FILES", path);
            }
            command.output().is_ok_and(|out| {
                String::from_utf8_lossy(&out.stdout).contains("Vulkan0:")
                    || String::from_utf8_lossy(&out.stderr).contains("Vulkan0:")
            })
        };
        if probe(None) {
            return Ok(None);
        }
        if std::env::var_os("VK_DRIVER_FILES").is_none() {
            let windows = PathBuf::from(
                std::env::var_os("SystemRoot").unwrap_or_else(|| "C:\\Windows".into()),
            );
            let repository = windows.join("System32/DriverStore/FileRepository");
            if let Ok(folders) = fs::read_dir(repository) {
                for folder in folders
                    .flatten()
                    .filter(|f| f.file_name().to_string_lossy().starts_with("nv"))
                {
                    let manifest = folder.path().join("nv-vk64.json");
                    if manifest.is_file() && probe(Some(&manifest)) {
                        return Ok(Some(manifest));
                    }
                }
            }
        }
        Err("GPU Vulkan недоступна: установите Vulkan-драйвер видеокарты или выберите CPU".into())
    }
    /// Creates an unloaded grammar model manager in the application's data directory.
    pub fn new(root: PathBuf) -> Self {
        Self {
            root,
            server: Arc::new(Mutex::new(None)),
            installing: Arc::new(AtomicBool::new(false)),
        }
    }
    /// Checks only the selected pinned weights and selected runtime installation.
    pub fn installed(&self, settings: &TranslatorSettings) -> bool {
        let Some(model) = catalog().into_iter().find(|m| m.id == settings.mini_model) else {
            return false;
        };
        self.root
            .join(&model.id)
            .join("model.gguf")
            .metadata()
            .is_ok_and(|m| m.len() == model.size)
            && self.engine(settings).join("llama-server.exe").exists()
    }
    /// Reports a live owned server, discarding already exited children.
    pub fn running(&self) -> bool {
        let mut server = self.server.lock().unwrap();
        if server
            .as_mut()
            .is_some_and(|s| matches!(s.child.try_wait(), Ok(Some(_))))
        {
            *server = None;
        }
        server.is_some()
    }
    /// Unloads the model owned by this manager, freeing GPU and CPU memory.
    pub fn stop(&self) {
        self.server.lock().unwrap().take();
    }
    /// Selects Vulkan for explicit GPU requests; CPU needs no GPU runtime.
    fn engine(&self, settings: &TranslatorSettings) -> PathBuf {
        self.root.join(if settings.device == "vulkan" {
            "engine-vulkan"
        } else {
            "engine-cpu"
        })
    }
    /// Downloads a hash-verified preset and the exact CPU or Vulkan runtime requested.
    pub async fn install(
        &self,
        app: &AppHandle,
        settings: &TranslatorSettings,
    ) -> Result<(), String> {
        if self.installing.swap(true, Ordering::SeqCst) {
            return Err("Установка модели уже идёт".into());
        }
        let result=async{
            let model=catalog().into_iter().find(|m|m.id==settings.mini_model).ok_or("Неизвестная встроенная модель")?;
            let directory=self.root.join(&model.id);fs::create_dir_all(&directory).map_err(|e|e.to_string())?;
            mini::download(app,&model.url,&model.hash,&directory.join("model.gguf"),&model.name).await?;
            let (url,hash)=if settings.device=="vulkan" {("https://github.com/ggml-org/llama.cpp/releases/download/b11351/llama-b11351-bin-win-vulkan-x64.zip","72a94b243aabc5715b177c1aacaf7729809d319ab3f88a8d6bdade41bb2f0440")}else{("https://github.com/ggml-org/llama.cpp/releases/download/b11351/llama-b11351-bin-win-cpu-x64.zip","ced25d91ed2c0981420dcfc9e7823892156c13b5c8dc9f26d334d8e3e4c33a09")};
            let archive=if settings.device=="vulkan"{"vulkan.zip"}else{"cpu.zip"};
            mini::download(app,url,hash,&self.root.join(archive),"llama.cpp runtime").await?;
            let root=self.root.clone();let destination=if settings.device=="vulkan"{"engine-vulkan"}else{"engine-cpu"};
            tauri::async_runtime::spawn_blocking(move||mini::extract(&root,archive,destination,false)).await.map_err(|e|e.to_string())??;
            let crt=app.path().resource_dir().map_err(|e|e.to_string())?.join("runtime/windows-x64");
            if crt.exists(){mini::copy_tree(&crt,&self.engine(settings))?;}
            fs::write(directory.join("SOURCES.txt"),format!("{}\n{}\nApache 2.0 model; llama.cpp b11351 MIT: https://github.com/ggml-org/llama.cpp/releases/tag/b11351\n",model.source,model.url)).map_err(|e|e.to_string())?;
            Ok(())
        }.await;
        self.installing.store(false, Ordering::SeqCst);
        result
    }
    /// Starts a private CPU or Vulkan server with bounded context and disabled thinking.
    pub fn start(&self, settings: &TranslatorSettings) -> Result<(String, String), String> {
        if settings.device == "vulkan"
            && !catalog()
                .iter()
                .any(|m| m.id == settings.mini_model && m.gpu)
        {
            return Err(
                "Эта модель в текущем runtime поддерживает CPU. Для GPU выберите Qwen3 4B.".into(),
            );
        }
        if !self.installed(settings) {
            return Err("Установите выбранную модель и runtime в настройках".into());
        }
        let signature = format!(
            "{}:{}:{}",
            settings.mini_model,
            settings.device,
            threads(settings)
        );
        let mut server = self.server.lock().unwrap();
        if let Some(s) = server.as_mut() {
            if s.child.try_wait().map_err(|e| e.to_string())?.is_none()
                && s.configuration == signature
            {
                return Ok((s.endpoint.clone(), s.token.clone()));
            }
            *server = None;
        }
        let port = TcpListener::bind("127.0.0.1:0")
            .map_err(|e| e.to_string())?
            .local_addr()
            .map_err(|e| e.to_string())?
            .port();
        let mut bytes = [0u8; 32];
        getrandom::fill(&mut bytes).map_err(|e| e.to_string())?;
        let token = bytes.iter().map(|b| format!("{b:02x}")).collect::<String>();
        let key_path = self.root.join("server-key.txt");
        fs::write(&key_path, &token).map_err(|e| e.to_string())?;
        let log =
            fs::File::create(self.root.join("grammar-server.log")).map_err(|e| e.to_string())?;
        let manifest = if settings.device == "vulkan" {
            self.vulkan_manifest(settings)?
        } else {
            None
        };
        let mut command = Command::new(self.engine(settings).join("llama-server.exe"));
        command.current_dir(self.engine(settings));
        if let Some(manifest) = manifest {
            command.env("VK_DRIVER_FILES", manifest);
        }
        if settings.device == "vulkan" {
            command.args(["--device", "Vulkan0"]);
        }
        let mut child = command
            .arg("--model")
            .arg(self.root.join(&settings.mini_model).join("model.gguf"))
            .args([
                "--host",
                "127.0.0.1",
                "--port",
                &port.to_string(),
                "--ctx-size",
                "4096",
                "--parallel",
                "1",
                "--threads",
                &threads(settings).to_string(),
                "--threads-batch",
                &threads(settings).to_string(),
                "--n-gpu-layers",
                if settings.device == "vulkan" {
                    "99"
                } else {
                    "0"
                },
                "--batch-size",
                "256",
                "--ubatch-size",
                "128",
                "--no-webui",
                "--reasoning",
                "off",
                "--api-key-file",
            ])
            .arg(&key_path)
            .stdout(Stdio::from(log.try_clone().map_err(|e| e.to_string())?))
            .stderr(Stdio::from(log))
            .creation_flags(0x08004000)
            .spawn()
            .map_err(|e| e.to_string())?;
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
        let endpoint = format!("http://127.0.0.1:{port}/v1");
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
}
