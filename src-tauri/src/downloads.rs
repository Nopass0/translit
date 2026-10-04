//! Resumable, bounded downloads committed only after checking the pinned SHA-256.
use reqwest::{header, Client, StatusCode};
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::{Read, Write},
    path::Path,
    time::{Duration, Instant},
};

/// Computes a digest without allocating a model-sized buffer.
pub fn hash_file(path: &Path) -> Result<String, String> {
    let mut file = fs::File::open(path).map_err(|e| e.to_string())?;
    let mut hash = Sha256::new();
    let mut buffer = [0; 65536];
    loop {
        let count = file.read(&mut buffer).map_err(|e| e.to_string())?;
        if count == 0 {
            break;
        }
        hash.update(&buffer[..count]);
    }
    Ok(format!("{:x}", hash.finalize()))
}

/// Checks existing bytes on a worker thread instead of blocking the UI executor.
async fn verified(path: &Path, expected: &str) -> Result<bool, String> {
    if !path.is_file() {
        return Ok(false);
    }
    let path = path.to_owned();
    let expected = expected.to_owned();
    tauri::async_runtime::spawn_blocking(move || hash_file(&path).map(|h| h == expected))
        .await
        .map_err(|e| e.to_string())?
}

/// Downloads one attempt, resuming a partial response when the server supports ranges.
async fn transfer(
    client: &Client,
    url: &str,
    partial: &Path,
    progress: &impl Fn(u64, u64),
) -> Result<(), String> {
    let offset = partial.metadata().map(|m| m.len()).unwrap_or(0);
    let mut request = client.get(url).header(header::ACCEPT_ENCODING, "identity");
    if offset > 0 {
        request = request.header(header::RANGE, format!("bytes={offset}-"));
    }
    let mut response = request
        .send()
        .await
        .map_err(|e| format!("Ошибка загрузки: {e}"))?;
    if response.status() == StatusCode::RANGE_NOT_SATISFIABLE {
        fs::File::create(partial).map_err(|e| e.to_string())?;
        return Err("Сервер отклонил продолжение загрузки; начинаю заново".into());
    }
    response.error_for_status_ref().map_err(|e| e.to_string())?;
    let resumed = response.status() == StatusCode::PARTIAL_CONTENT;
    let content_range = response
        .headers()
        .get(header::CONTENT_RANGE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    if resumed && !content_range.starts_with(&format!("bytes {offset}-")) {
        fs::File::create(partial).map_err(|e| e.to_string())?;
        return Err("Некорректный диапазон загрузки".into());
    }
    let mut received = if resumed { offset } else { 0 };
    let total = content_range
        .rsplit('/')
        .next()
        .and_then(|s| s.parse().ok())
        .unwrap_or_else(|| response.content_length().map(|n| n + received).unwrap_or(0));
    let mut file = fs::OpenOptions::new()
        .create(true)
        .write(true)
        .append(resumed)
        .truncate(!resumed)
        .open(partial)
        .map_err(|e| e.to_string())?;
    progress(received, total);
    let mut last = Instant::now();
    while let Some(chunk) = response.chunk().await.map_err(|e| e.to_string())? {
        file.write_all(&chunk).map_err(|e| e.to_string())?;
        received += chunk.len() as u64;
        if last.elapsed() >= Duration::from_millis(250) {
            progress(received, total);
            last = Instant::now();
        }
    }
    file.sync_all().map_err(|e| e.to_string())?;
    progress(received, total);
    Ok(())
}

/// Retries interruptions three times and preserves partial bytes across application restarts.
pub async fn download(
    url: &str,
    expected: &str,
    path: &Path,
    progress: impl Fn(u64, u64),
) -> Result<(), String> {
    if verified(path, expected).await? {
        return Ok(());
    }
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let partial = path.with_extension("download");
    let client = Client::builder()
        .connect_timeout(Duration::from_secs(20))
        .read_timeout(Duration::from_secs(60))
        .timeout(Duration::from_secs(1800))
        .build()
        .map_err(|e| e.to_string())?;
    let mut error = String::new();
    for attempt in 0..3 {
        if !verified(&partial, expected).await? {
            match transfer(&client, url, &partial, &progress).await {
                Ok(()) => {
                    if !verified(&partial, expected).await? {
                        fs::remove_file(&partial).map_err(|e| e.to_string())?;
                        error = "Контрольная сумма модели/движка не совпала".into();
                    } else {
                        return fs::rename(&partial, path).map_err(|e| e.to_string());
                    }
                }
                Err(reason) => error = reason,
            }
        } else {
            return fs::rename(&partial, path).map_err(|e| e.to_string());
        }
        if attempt < 2 {
            tokio::time::sleep(Duration::from_secs(attempt + 1)).await;
        }
    }
    Err(format!(
        "{error}. Незавершённая загрузка сохранена; повторите установку."
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{net::TcpListener, thread};

    /// An interrupted response must resume its bytes, verify its digest, and reuse the final file.
    #[test]
    fn interrupted_download_resumes_and_cached_file_is_reused() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/model", listener.local_addr().unwrap());
        let payload = vec![b'q'; 8192];
        let expected = format!("{:x}", Sha256::digest(&payload));
        let server = thread::spawn(move || {
            for attempt in 0..2 {
                let (mut stream, _) = listener.accept().unwrap();
                stream
                    .set_read_timeout(Some(Duration::from_secs(10)))
                    .unwrap();
                let mut request = Vec::new();
                while !request.ends_with(b"\r\n\r\n") {
                    let mut byte = [0];
                    stream.read_exact(&mut byte).unwrap();
                    request.push(byte[0]);
                }
                let request = String::from_utf8(request).unwrap().to_lowercase();
                if attempt == 0 {
                    write!(
                        stream,
                        "HTTP/1.1 200 OK\r\nContent-Length: 8192\r\nConnection: close\r\n\r\n"
                    )
                    .unwrap();
                    stream.write_all(&payload[..1024]).unwrap();
                    stream.flush().unwrap();
                    thread::sleep(Duration::from_millis(40));
                } else {
                    assert!(request.contains("range: bytes=1024-"), "{request}");
                    write!(stream, "HTTP/1.1 206 Partial Content\r\nContent-Length: 7168\r\nContent-Range: bytes 1024-8191/8192\r\nConnection: close\r\n\r\n").unwrap();
                    stream.write_all(&payload[1024..]).unwrap();
                }
            }
        });
        let directory =
            std::env::temp_dir().join(format!("translit-download-test-{}", std::process::id()));
        let path = directory.join("model.gguf");
        fs::create_dir_all(&directory).unwrap();
        let _ = fs::remove_file(&path);
        let _ = fs::remove_file(path.with_extension("download"));
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        runtime
            .block_on(download(&url, &expected, &path, |_, _| {}))
            .unwrap();
        server.join().unwrap();
        assert_eq!(path.metadata().unwrap().len(), 8192);
        assert!(!path.with_extension("download").exists());
        runtime
            .block_on(download(
                "http://127.0.0.1:1/unused",
                &expected,
                &path,
                |_, _| {},
            ))
            .unwrap();
        fs::remove_file(path).unwrap();
        fs::remove_dir(directory).unwrap();
    }
}
