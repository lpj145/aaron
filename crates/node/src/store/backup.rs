use crate::BoxError;
use std::path::Path;

pub(crate) fn copy_dir_all(src: &Path, dst: &Path) -> Result<(), BoxError> {
    if src == dst {
        return Err(Box::new(std::io::Error::other(
            "source and destination directories cannot be the same",
        )));
    }
    std::fs::create_dir_all(dst)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let file_type = entry.file_type()?;
        let dest_path = dst.join(entry.file_name());

        if file_type.is_dir() {
            copy_dir_all(&entry.path(), &dest_path)?;
        } else if file_type.is_file() {
            // Skip temporary lock files if present
            if entry.file_name() == "flock" {
                continue;
            }
            std::fs::copy(entry.path(), dest_path)?;
        }
    }
    Ok(())
}
