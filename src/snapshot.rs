use std::path::Path;

use ron::error::SpannedError;
use thiserror::Error;
use tokio::fs::{remove_file, rename, File};
use tokio::io::{AsyncReadExt, AsyncWriteExt, BufWriter};

use crate::data::registry::ValueSnapshots;

pub async fn read_snapshot(
    file: impl AsRef<Path>,
) -> Result<Option<ValueSnapshots>, SnapshotAccessError> {
    Ok(if file.as_ref().exists() {
        let mut file = File::open(&file)
            .await
            .map_err(SnapshotAccessError::OpenFile)?;
        let mut content = String::new();
        file.read_to_string(&mut content)
            .await
            .map_err(SnapshotAccessError::ReadFile)?;
        Some(ron::from_str(&content).map_err(SnapshotAccessError::Deserialize)?)
    } else {
        None
    })
}

pub async fn write_snapshot(
    snapshot: &ValueSnapshots,
    file: impl AsRef<Path>,
) -> Result<(), SnapshotAccessError> {
    let ron_content = ron::ser::to_string(&snapshot).map_err(SnapshotAccessError::Serialize)?;
    let temp_filename = file.as_ref().with_extension("tmp");
    if temp_filename.exists() {
        remove_file(&temp_filename)
            .await
            .map_err(SnapshotAccessError::Remove)?;
    }
    let tempfile = File::create(&temp_filename)
        .await
        .map_err(SnapshotAccessError::CreateFile)?;
    let mut writer = BufWriter::new(tempfile);
    writer
        .write(ron_content.as_bytes())
        .await
        .map_err(SnapshotAccessError::WriteFile)?;
    writer
        .flush()
        .await
        .map_err(SnapshotAccessError::WriteFile)?;
    drop(writer);
    rename(&temp_filename, file)
        .await
        .map_err(SnapshotAccessError::Rename)?;
    Ok(())
}

#[derive(Error, Debug)]
pub enum SnapshotAccessError {
    #[error("Cannot open file: {0}")]
    OpenFile(std::io::Error),
    #[error("Cannot create file: {0}")]
    CreateFile(std::io::Error),
    #[error("Cannot serialize state {0}")]
    Serialize(ron::Error),
    #[error("Cannot write to file {0}")]
    WriteFile(std::io::Error),
    #[error("Cannot rename file {0}")]
    Rename(std::io::Error),
    #[error("Cannot remove temp file before write {0}")]
    Remove(std::io::Error),
    #[error("Cannot read file {0}")]
    ReadFile(std::io::Error),
    #[error("Cannot deserialize state {0}")]
    Deserialize(SpannedError),
}
