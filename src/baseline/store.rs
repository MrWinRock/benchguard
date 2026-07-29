use std::{
    fs,
    io::{self, Write},
    path::Path,
};

use tempfile::NamedTempFile;

use crate::{baseline::schema::BaselineFileV1, error::BenchguardError};

pub struct BaselineStore;

impl BaselineStore {
    pub fn load(path: &Path) -> Result<BaselineFileV1, BenchguardError> {
        let bytes = fs::read(path).map_err(|source| baseline_io("read baseline", path, source))?;
        let value: BaselineFileV1 = serde_json::from_slice(&bytes).map_err(|source| {
            BenchguardError::InvalidBaseline(format!(
                "failed to parse baseline {}: {source}",
                path.display()
            ))
        })?;

        match value.validate() {
            Err(BenchguardError::InvalidBaseline(message)) => {
                Err(BenchguardError::InvalidBaseline(format!(
                    "baseline {} failed validation: {message}",
                    path.display()
                )))
            }
            Err(error) => Err(error),
            Ok(()) => Ok(value),
        }
    }

    pub fn save_atomic(path: &Path, value: &BaselineFileV1) -> Result<(), BenchguardError> {
        Self::save_atomic_with(path, value, &ProductionAtomicReplace)
    }

    fn save_atomic_with(
        path: &Path,
        value: &BaselineFileV1,
        replacer: &dyn AtomicReplace,
    ) -> Result<(), BenchguardError> {
        value.validate()?;
        let bytes =
            serde_json::to_vec_pretty(value).map_err(BenchguardError::BaselineSerialization)?;

        let parent = path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new("."));
        let mut temporary = NamedTempFile::new_in(parent)
            .map_err(|source| baseline_io("create temporary baseline", path, source))?;
        temporary
            .write_all(&bytes)
            .map_err(|source| baseline_io("write temporary baseline", path, source))?;
        temporary
            .flush()
            .map_err(|source| baseline_io("flush temporary baseline", path, source))?;
        temporary
            .as_file()
            .sync_all()
            .map_err(|source| baseline_io("sync temporary baseline", path, source))?;
        let mut temporary_path = temporary.into_temp_path();
        match replacer.replace(temporary_path.as_ref(), path) {
            Ok(()) => {
                temporary_path.disable_cleanup(true);
                Ok(())
            }
            Err(source) => Err(baseline_io("replace baseline", path, source)),
        }
    }
}

fn baseline_io(operation: &'static str, path: &Path, source: io::Error) -> BenchguardError {
    BenchguardError::BaselineIo {
        operation,
        path: path.to_owned(),
        source,
    }
}

trait AtomicReplace {
    fn replace(&self, source: &Path, destination: &Path) -> io::Result<()>;
}

struct ProductionAtomicReplace;

#[cfg(target_os = "linux")]
impl AtomicReplace for ProductionAtomicReplace {
    fn replace(&self, source: &Path, destination: &Path) -> io::Result<()> {
        fs::rename(source, destination)
    }
}

#[cfg(windows)]
impl AtomicReplace for ProductionAtomicReplace {
    fn replace(&self, source: &Path, destination: &Path) -> io::Result<()> {
        replace_file_windows(source, destination)
    }
}

#[cfg(not(any(target_os = "linux", windows)))]
impl AtomicReplace for ProductionAtomicReplace {
    fn replace(&self, source: &Path, destination: &Path) -> io::Result<()> {
        fs::rename(source, destination)
    }
}

#[cfg(windows)]
fn replace_file_windows(source: &Path, destination: &Path) -> io::Result<()> {
    replace_file_windows_with(source, destination, &Win32FileOps)
}

#[cfg(windows)]
trait WindowsFileOps {
    fn set_file_attributes_normal(&self, path: &Path) -> io::Result<()>;

    fn replace_with_backup(
        &self,
        source: &Path,
        destination: &Path,
        backup: &Path,
    ) -> io::Result<()>;

    fn move_write_through(
        &self,
        source: &Path,
        destination: &Path,
        replace_existing: bool,
    ) -> io::Result<()>;
}

#[cfg(windows)]
struct Win32FileOps;

#[cfg(windows)]
impl Win32FileOps {
    fn wide_path(path: &Path) -> io::Result<Vec<u16>> {
        use std::os::windows::ffi::OsStrExt;

        const SEPARATOR: u16 = b'\\' as u16;
        const QUESTION_MARK: u16 = b'?' as u16;
        const DOT: u16 = b'.' as u16;
        const VERBATIM_PREFIX: &[u16] = &[SEPARATOR, SEPARATOR, QUESTION_MARK, SEPARATOR];
        const DEVICE_PREFIX: &[u16] = &[SEPARATOR, SEPARATOR, DOT, SEPARATOR];
        const UNC_PREFIX: &[u16] = &[
            SEPARATOR,
            SEPARATOR,
            QUESTION_MARK,
            SEPARATOR,
            b'U' as u16,
            b'N' as u16,
            b'C' as u16,
            SEPARATOR,
        ];

        let absolute = std::path::absolute(path)?;
        let wide: Vec<u16> = absolute.as_os_str().encode_wide().collect();
        if wide.contains(&0) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "path contains an interior NUL",
            ));
        }

        let mut extended = if wide.starts_with(VERBATIM_PREFIX) || wide.starts_with(DEVICE_PREFIX) {
            wide
        } else if wide.starts_with(&[SEPARATOR, SEPARATOR]) {
            let mut extended = UNC_PREFIX.to_vec();
            extended.extend_from_slice(&wide[2..]);
            extended
        } else {
            let mut extended = VERBATIM_PREFIX.to_vec();
            extended.extend_from_slice(&wide);
            extended
        };
        extended.push(0);
        Ok(extended)
    }
}

#[cfg(windows)]
impl WindowsFileOps for Win32FileOps {
    fn set_file_attributes_normal(&self, path: &Path) -> io::Result<()> {
        use windows_sys::Win32::Storage::FileSystem::{FILE_ATTRIBUTE_NORMAL, SetFileAttributesW};

        let path = Self::wide_path(path)?;
        if unsafe { SetFileAttributesW(path.as_ptr(), FILE_ATTRIBUTE_NORMAL) } == 0 {
            Err(io::Error::last_os_error())
        } else {
            Ok(())
        }
    }

    fn replace_with_backup(
        &self,
        source: &Path,
        destination: &Path,
        backup: &Path,
    ) -> io::Result<()> {
        use std::ptr;

        use windows_sys::Win32::Storage::FileSystem::ReplaceFileW;

        let source = Self::wide_path(source)?;
        let destination = Self::wide_path(destination)?;
        let backup = Self::wide_path(backup)?;
        if unsafe {
            ReplaceFileW(
                destination.as_ptr(),
                source.as_ptr(),
                backup.as_ptr(),
                0,
                ptr::null(),
                ptr::null(),
            )
        } == 0
        {
            Err(io::Error::last_os_error())
        } else {
            Ok(())
        }
    }

    fn move_write_through(
        &self,
        source: &Path,
        destination: &Path,
        replace_existing: bool,
    ) -> io::Result<()> {
        use windows_sys::Win32::Storage::FileSystem::{
            MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH, MoveFileExW,
        };

        let source = Self::wide_path(source)?;
        let destination = Self::wide_path(destination)?;
        let mut flags = MOVEFILE_WRITE_THROUGH;
        if replace_existing {
            flags |= MOVEFILE_REPLACE_EXISTING;
        }
        if unsafe { MoveFileExW(source.as_ptr(), destination.as_ptr(), flags) } == 0 {
            Err(io::Error::last_os_error())
        } else {
            Ok(())
        }
    }
}

#[cfg(windows)]
fn replace_file_windows_with(
    source: &Path,
    destination: &Path,
    file_ops: &dyn WindowsFileOps,
) -> io::Result<()> {
    use windows_sys::Win32::Foundation::ERROR_UNABLE_TO_MOVE_REPLACEMENT_2;

    file_ops.set_file_attributes_normal(source)?;
    if !destination.try_exists()? {
        return file_ops.move_write_through(source, destination, false);
    }

    let parent = destination
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let backup_file = tempfile::Builder::new()
        .prefix(".benchguard-backup-")
        .tempfile_in(parent)?;
    let backup = backup_file.path().to_owned();
    backup_file.close()?;

    match file_ops.replace_with_backup(source, destination, &backup) {
        Ok(()) => {
            let _ = fs::remove_file(backup);
            Ok(())
        }
        // With a backup, error 1176 and all non-partial failures retain the
        // original names. Error 1177 is the documented partial state: the old
        // destination has moved to the backup while the replacement remains.
        Err(replace_error)
            if replace_error.raw_os_error()
                == i32::try_from(ERROR_UNABLE_TO_MOVE_REPLACEMENT_2).ok() =>
        {
            match file_ops.move_write_through(&backup, destination, true) {
                Ok(()) => Err(replace_error),
                Err(recovery_error) => Err(io::Error::new(
                    recovery_error.kind(),
                    format!(
                        "atomic replacement failed ({replace_error}); restoring backup {} failed \
                         ({recovery_error}); the prior baseline remains at {}",
                        backup.display(),
                        backup.display()
                    ),
                )),
            }
        }
        Err(error) => Err(error),
    }
}

#[cfg(test)]
mod tests {
    use std::{
        cell::RefCell,
        collections::BTreeMap,
        io,
        path::{Path, PathBuf},
    };

    use super::{AtomicReplace, BaselineStore};
    #[cfg(windows)]
    use super::{Win32FileOps, WindowsFileOps, replace_file_windows_with};
    use crate::{
        baseline::schema::{
            BaselineFileV1, BenchmarkV1, BudgetsV1, MetricAggregateV1, NoiseFloorsV1,
        },
        domain::PlatformId,
        error::BenchguardError,
    };

    fn example_aggregate() -> MetricAggregateV1 {
        MetricAggregateV1 {
            median: 1_000,
            mean: 1_010,
            standard_deviation: 25,
            min: 950,
            max: 1_100,
            p50: 1_000,
            p95: 1_100,
            sample_count: 3,
        }
    }

    fn example_baseline() -> BaselineFileV1 {
        BaselineFileV1 {
            schema_version: 1,
            benchmarks: BTreeMap::from([(
                "startup".to_owned(),
                BenchmarkV1 {
                    program: "benchguard-fixture".to_owned(),
                    args: vec!["sleep-ms".to_owned(), "10".to_owned()],
                    recorded_at: "2026-07-28T12:34:56Z".to_owned(),
                    platform: PlatformId {
                        os: "windows".to_owned(),
                        arch: "x86_64".to_owned(),
                    },
                    benchguard_version: "0.1.0".to_owned(),
                    warmups: 2,
                    runs: 3,
                    timeout_ns: Some(5_000_000_000),
                    wall_ns: example_aggregate(),
                    cpu_ns: example_aggregate(),
                    peak_memory_bytes: example_aggregate(),
                    budgets: BudgetsV1 {
                        wall_percent: Some(10.0),
                        cpu_percent: None,
                        peak_memory_percent: Some(15.5),
                    },
                    noise_floors: NoiseFloorsV1 {
                        wall_ns: 1_000_000,
                        cpu_ns: 500_000,
                        peak_memory_bytes: 1_048_576,
                    },
                },
            )]),
        }
    }

    #[test]
    fn save_and_load_preserve_a_valid_baseline() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("benchguard.json");
        let expected = example_baseline();

        BaselineStore::save_atomic(&path, &expected).unwrap();

        assert_eq!(BaselineStore::load(&path).unwrap(), expected);
    }

    // Catches a Windows replacement path that works for first creation but
    // cannot replace a real existing destination through the production API.
    #[test]
    fn save_replaces_an_existing_valid_baseline() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("benchguard.json");
        let mut updated = example_baseline();
        let aggregate = &mut updated.benchmarks.get_mut("startup").unwrap().wall_ns;
        aggregate.median = 2_000;
        aggregate.mean = 2_000;
        aggregate.standard_deviation = 0;
        aggregate.min = 2_000;
        aggregate.max = 2_000;
        aggregate.p50 = 2_000;
        aggregate.p95 = 2_000;

        BaselineStore::save_atomic(&path, &example_baseline()).unwrap();
        BaselineStore::save_atomic(&path, &updated).unwrap();

        assert_eq!(BaselineStore::load(&path).unwrap(), updated);
    }

    #[test]
    fn invalid_value_preserves_existing_bytes() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("benchguard.json");
        std::fs::write(&path, b"previous-valid-content").unwrap();
        let mut invalid = example_baseline();
        invalid.schema_version = 9;

        let result = BaselineStore::save_atomic(&path, &invalid);

        assert!(result.is_err());
        assert_eq!(std::fs::read(&path).unwrap(), b"previous-valid-content");
    }

    struct FailingAtomicReplace;

    impl AtomicReplace for FailingAtomicReplace {
        fn replace(&self, _source: &Path, _destination: &Path) -> io::Result<()> {
            Err(io::Error::other("injected replacement failure"))
        }
    }

    #[test]
    fn replacement_failure_preserves_existing_bytes() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("benchguard.json");
        std::fs::write(&path, b"previous-valid-content").unwrap();

        let result =
            BaselineStore::save_atomic_with(&path, &example_baseline(), &FailingAtomicReplace);

        assert!(matches!(
            result,
            Err(BenchguardError::BaselineIo {
                operation: "replace baseline",
                path: error_path,
                source,
            }) if error_path == path && source.to_string() == "injected replacement failure"
        ));
        assert_eq!(std::fs::read(&path).unwrap(), b"previous-valid-content");
    }

    struct RecreatingAtomicReplace {
        recreated_path: RefCell<Option<PathBuf>>,
    }

    impl AtomicReplace for RecreatingAtomicReplace {
        fn replace(&self, source: &Path, destination: &Path) -> io::Result<()> {
            std::fs::rename(source, destination)?;
            std::fs::write(source, b"racing-file-content")?;
            self.recreated_path.replace(Some(source.to_owned()));
            Ok(())
        }
    }

    #[test]
    fn successful_replacement_does_not_delete_a_file_recreated_at_the_temp_path() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("benchguard.json");
        let replacer = RecreatingAtomicReplace {
            recreated_path: RefCell::new(None),
        };

        BaselineStore::save_atomic_with(&path, &example_baseline(), &replacer).unwrap();

        let recreated_path = replacer.recreated_path.borrow().clone().unwrap();
        assert_eq!(
            std::fs::read(recreated_path).unwrap(),
            b"racing-file-content"
        );
        assert_eq!(BaselineStore::load(&path).unwrap(), example_baseline());
    }

    #[test]
    fn malformed_json_is_an_invalid_baseline() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("benchguard.json");
        std::fs::write(&path, b"{not json").unwrap();

        let result = BaselineStore::load(&path);

        assert!(matches!(
            result,
            Err(BenchguardError::InvalidBaseline(message))
                if message.contains("failed to parse baseline")
                    && message.contains("benchguard.json")
        ));
    }

    #[test]
    fn missing_baseline_reports_the_read_path_and_source() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("missing.json");

        let result = BaselineStore::load(&path);

        assert!(matches!(
            result,
            Err(BenchguardError::BaselineIo {
                operation: "read baseline",
                path: error_path,
                source,
            }) if error_path == path && source.kind() == io::ErrorKind::NotFound
        ));
    }

    #[test]
    fn loaded_unsupported_schema_remains_distinct() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("benchguard.json");
        let mut unsupported = example_baseline();
        unsupported.schema_version = 9;
        std::fs::write(&path, serde_json::to_vec_pretty(&unsupported).unwrap()).unwrap();

        let result = BaselineStore::load(&path);

        assert!(matches!(result, Err(BenchguardError::UnsupportedSchema(9))));
    }

    #[cfg(windows)]
    struct PartialFailureWindowsFileOps {
        error_code: i32,
    }

    #[cfg(windows)]
    impl WindowsFileOps for PartialFailureWindowsFileOps {
        fn set_file_attributes_normal(&self, _path: &Path) -> io::Result<()> {
            Ok(())
        }

        fn replace_with_backup(
            &self,
            _source: &Path,
            destination: &Path,
            backup: &Path,
        ) -> io::Result<()> {
            if self.error_code == 1177 {
                std::fs::rename(destination, backup)?;
            }
            Err(io::Error::from_raw_os_error(self.error_code))
        }

        fn move_write_through(
            &self,
            source: &Path,
            destination: &Path,
            _replace_existing: bool,
        ) -> io::Result<()> {
            std::fs::rename(source, destination)
        }
    }

    #[cfg(windows)]
    struct InjectedWindowsAtomicReplace {
        file_ops: PartialFailureWindowsFileOps,
    }

    #[cfg(windows)]
    impl AtomicReplace for InjectedWindowsAtomicReplace {
        fn replace(&self, source: &Path, destination: &Path) -> io::Result<()> {
            replace_file_windows_with(source, destination, &self.file_ops)
        }
    }

    #[cfg(windows)]
    #[test]
    fn windows_error_1176_preserves_existing_destination_bytes() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("benchguard.json");
        std::fs::write(&path, b"previous-valid-content").unwrap();
        let replacer = InjectedWindowsAtomicReplace {
            file_ops: PartialFailureWindowsFileOps { error_code: 1176 },
        };

        let result = BaselineStore::save_atomic_with(&path, &example_baseline(), &replacer);

        assert!(matches!(
            result,
            Err(BenchguardError::BaselineIo { source, .. })
                if source.raw_os_error() == Some(1176)
        ));
        assert_eq!(std::fs::read(&path).unwrap(), b"previous-valid-content");
    }

    #[cfg(windows)]
    #[test]
    fn windows_error_1177_restores_existing_destination_bytes_from_backup() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("benchguard.json");
        std::fs::write(&path, b"previous-valid-content").unwrap();
        let replacer = InjectedWindowsAtomicReplace {
            file_ops: PartialFailureWindowsFileOps { error_code: 1177 },
        };

        let result = BaselineStore::save_atomic_with(&path, &example_baseline(), &replacer);

        assert!(result.is_err());
        assert_eq!(std::fs::read(&path).unwrap(), b"previous-valid-content");
    }

    #[cfg(windows)]
    #[test]
    fn windows_long_absolute_paths_use_the_extended_length_prefix() {
        let path = format!(r"C:\{}benchguard.json", "deep-directory\\".repeat(20));
        assert!(path.encode_utf16().count() > 260);

        let wide = Win32FileOps::wide_path(Path::new(&path)).unwrap();
        let converted = String::from_utf16(&wide[..wide.len() - 1]).unwrap();

        assert_eq!(wide.last(), Some(&0));
        assert_eq!(converted, format!(r"\\?\{path}"));
    }
}
