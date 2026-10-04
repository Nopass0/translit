//! Windows OCR with an application-local English fallback for clean installations.
use crate::{
    models::{Ocr, Word},
    native,
};
use std::{collections::HashMap, os::windows::process::CommandExt, path::Path, process::Command};

/// Recognizes a captured BMP without requiring a system Python or language pack.
pub fn recognize(helper: &Path, image: &Path) -> Result<Ocr, String> {
    if let Ok(json) = native::call(
        helper,
        &["ocr".into(), image.to_string_lossy().into_owned()],
    ) {
        if let Ok(ocr) = serde_json::from_str::<Ocr>(&json) {
            return Ok(ocr);
        }
    }
    let root = helper
        .parent()
        .and_then(Path::parent)
        .ok_or("Не найден каталог OCR")?
        .join("ocr");
    tesseract(&root, image)
}

/// Parses Tesseract TSV into the same coordinates and line IDs as Windows OCR.
pub fn tesseract(root: &Path, image: &Path) -> Result<Ocr, String> {
    let output = Command::new(root.join("tesseract.exe"))
        .arg(image)
        .args([
            "stdout",
            "-l",
            "eng",
            "--psm",
            "11",
            "-c",
            "tessedit_create_tsv=1",
        ])
        .arg("--tessdata-dir")
        .arg(root.join("tessdata"))
        .env("OMP_THREAD_LIMIT", "2")
        .creation_flags(0x08000000)
        .output()
        .map_err(|e| format!("Не удалось запустить встроенный OCR: {e}"))?;
    if !output.status.success() {
        return Err("Встроенный OCR не смог распознать кадр".into());
    }
    let (width, height) = image::image_dimensions(image).map_err(|e| e.to_string())?;
    let mut lines = HashMap::new();
    let mut words = Vec::new();
    for row in String::from_utf8_lossy(&output.stdout).lines().skip(1) {
        let columns = row.splitn(12, '\t').collect::<Vec<_>>();
        if columns.len() != 12 || columns[0] != "5" || columns[11].trim().is_empty() {
            continue;
        }
        let confidence = columns[10].parse::<f64>().unwrap_or(-1.0);
        if confidence < 25.0 {
            continue;
        }
        let key = format!(
            "{}:{}:{}:{}",
            columns[1], columns[2], columns[3], columns[4]
        );
        let next = lines.len() as u32;
        let line = *lines.entry(key).or_insert(next);
        let number = |index: usize| columns[index].parse::<f64>().unwrap_or_default();
        words.push(Word {
            text: columns[11].trim().into(),
            line,
            x: number(6),
            y: number(7),
            width: number(8),
            height: number(9),
        });
    }
    let mut text = String::new();
    let mut previous = None;
    for word in &words {
        if !text.is_empty() {
            text.push(if previous == Some(word.line) {
                ' '
            } else {
                '\n'
            });
        }
        text.push_str(&word.text);
        previous = Some(word.line);
    }
    Ok(Ocr {
        width,
        height,
        text,
        words,
    })
}
