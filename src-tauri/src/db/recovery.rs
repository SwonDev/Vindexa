use super::{
    Database, copy_all_pages_atomically, open_read_only_database, same_file_identity,
    validate_current_database,
};
use crate::error::{AppError, AppResult};
use crate::models::{
    DatabaseRecoveryIssue, DatabaseRecoverySnapshot, QuarantinedDatabaseSummary,
    RecoveryBackupSummary,
};
use chrono::{DateTime, Utc};
use rusqlite::Connection;
use std::fs::{self, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::SystemTime;
use uuid::Uuid;

const RESTORE_CONFIRMATION: &str = "RESTAURAR";
const CLEAN_CONFIRMATION: &str = "CREAR NUEVA";

#[derive(Debug, Clone)]
struct QuarantineRecord {
    summary: QuarantinedDatabaseSummary,
    database_path: PathBuf,
}

#[derive(Debug, Clone)]
struct RecoveryCandidate {
    summary: RecoveryBackupSummary,
    path: PathBuf,
}

#[derive(Debug)]
pub struct StartupRecovery {
    database: Database,
    data_directory: PathBuf,
    issue: Option<DatabaseRecoveryIssue>,
    quarantine: Option<QuarantineRecord>,
    candidates: Vec<RecoveryCandidate>,
    quarantine_failure: Option<String>,
}

impl StartupRecovery {
    pub fn prepare(database: Database) -> Self {
        let data_directory = database
            .path()
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .to_path_buf();
        match database.initialize() {
            Ok(()) => Self {
                database,
                data_directory,
                issue: None,
                quarantine: None,
                candidates: Vec::new(),
                quarantine_failure: None,
            },
            Err(error) => {
                database.set_available(false);
                let (quarantine, quarantine_failure) =
                    match quarantine_database(database.path(), &data_directory) {
                        Ok(record) => (Some(record), None),
                        Err(quarantine_error) => (None, Some(quarantine_error.message)),
                    };
                let mut recovery = Self {
                    database,
                    data_directory,
                    issue: Some(DatabaseRecoveryIssue {
                        code: error.code,
                        message: error.message,
                    }),
                    quarantine,
                    candidates: Vec::new(),
                    quarantine_failure,
                };
                recovery.refresh_candidates();
                recovery
            }
        }
    }

    pub fn snapshot(&self) -> DatabaseRecoverySnapshot {
        let issue = self.issue.clone().map(|mut issue| {
            if let Some(failure) = &self.quarantine_failure {
                issue.message = format!(
                    "{} No se pudo aislar el archivo automáticamente: {failure}",
                    issue.message
                );
            }
            issue
        });
        DatabaseRecoverySnapshot {
            required: self.issue.is_some(),
            issue,
            quarantine: self
                .quarantine
                .as_ref()
                .map(|record| record.summary.clone()),
            backups: self
                .candidates
                .iter()
                .map(|candidate| candidate.summary.clone())
                .collect(),
            recovery_actions_available: self.issue.is_some()
                && self.quarantine.is_some()
                && active_database_files_absent(self.database.path()),
        }
    }

    pub fn is_required(&self) -> bool {
        self.issue.is_some()
    }

    pub fn refresh_candidates(&mut self) {
        let selected = self
            .candidates
            .iter()
            .filter(|candidate| candidate.summary.source == "selected")
            .cloned()
            .collect::<Vec<_>>();
        self.candidates = discover_safety_candidates(&self.data_directory);
        for candidate in selected {
            if !self
                .candidates
                .iter()
                .any(|existing| existing.path == candidate.path)
            {
                self.candidates.push(candidate);
            }
        }
    }

    pub fn add_selected_candidate(&mut self, path: PathBuf) -> AppResult<()> {
        if !self.is_required() {
            return Err(recovery_not_required());
        }
        let candidate = inspect_candidate(path, "selected")?;
        self.candidates
            .retain(|existing| existing.path != candidate.path);
        self.candidates.push(candidate);
        Ok(())
    }

    pub fn restore(&mut self, candidate_id: &str, confirmation: &str) -> AppResult<()> {
        self.ensure_action_available()?;
        if confirmation != RESTORE_CONFIRMATION {
            return Err(AppError::new(
                "recovery_confirmation",
                "Escribe RESTAURAR para confirmar la recuperación.",
            ));
        }
        let candidate = self
            .candidates
            .iter()
            .find(|candidate| candidate.summary.id == candidate_id)
            .cloned()
            .ok_or_else(|| AppError::not_found("La copia seleccionada ya no está disponible."))?;
        if !candidate.summary.valid {
            return Err(AppError::new(
                "recovery_backup_invalid",
                "La copia no superó la validación de identidad, esquema e integridad.",
            ));
        }

        reject_unsafe_candidate_path(&candidate.path)?;
        let source = open_read_only_database(&candidate.path)?;
        validate_current_database(&source, "recovery_backup").map_err(|_| {
            AppError::new(
                "recovery_backup_invalid",
                "La copia cambió o dejó de ser válida desde su comprobación.",
            )
        })?;
        self.install_from_connection(&source)
    }

    pub fn create_clean(&mut self, confirmation: &str) -> AppResult<()> {
        self.ensure_action_available()?;
        if confirmation != CLEAN_CONFIRMATION {
            return Err(AppError::new(
                "recovery_confirmation",
                "Escribe CREAR NUEVA para confirmar una base local vacía.",
            ));
        }
        let temporary_path = self.temporary_database_path();
        let temporary = Database::new(temporary_path.clone());
        let result = (|| -> AppResult<()> {
            temporary.initialize()?;
            prepare_database_file_for_rename(&temporary_path)?;
            self.commit_temporary_database(&temporary_path)
        })();
        if result.is_err() {
            remove_temporary_database(&temporary_path);
        }
        result
    }

    #[cfg(test)]
    fn quarantine_exists(&self, id: &str) -> bool {
        self.quarantine
            .as_ref()
            .is_some_and(|record| record.summary.id == id && record.database_path.exists())
    }

    pub fn quarantine_source(&self) -> AppResult<&Path> {
        self.quarantine
            .as_ref()
            .map(|record| record.database_path.as_path())
            .filter(|path| path.is_file())
            .ok_or_else(|| AppError::not_found("El archivo en cuarentena ya no está disponible."))
    }

    pub fn export_quarantined_database(&self, destination: &Path) -> AppResult<()> {
        let source = self.quarantine_source()?;
        if let Ok(metadata) = fs::symlink_metadata(destination) {
            if metadata.file_type().is_symlink() {
                return Err(AppError::validation(
                    "El destino de diagnóstico no puede ser un enlace simbólico.",
                ));
            }
            if same_file_identity(source, destination) {
                return Err(AppError::new(
                    "recovery_export_alias",
                    "El destino apunta al mismo archivo físico que la cuarentena.",
                ));
            }
            return Err(AppError::validation(
                "El destino ya existe. Elige un nombre nuevo para no sobrescribir archivos.",
            ));
        }
        let parent = destination.parent().ok_or_else(|| {
            AppError::validation("El destino de diagnóstico no tiene un directorio válido.")
        })?;
        fs::create_dir_all(parent)?;
        if destination == source || same_file_identity(source, destination) {
            return Err(AppError::validation(
                "El archivo en cuarentena no puede sobrescribirse a sí mismo.",
            ));
        }
        let temporary = parent.join(format!(".vindexa-quarantine-export-{}.tmp", Uuid::new_v4()));
        let result = (|| -> AppResult<()> {
            let mut input = fs::File::open(source)?;
            let mut output = OpenOptions::new()
                .create_new(true)
                .write(true)
                .open(&temporary)?;
            let mut buffer = [0_u8; 64 * 1024];
            loop {
                let read = input.read(&mut buffer)?;
                if read == 0 {
                    break;
                }
                output.write_all(&buffer[..read])?;
            }
            output.sync_all()?;
            drop(output);
            fs::rename(&temporary, destination)?;
            Ok(())
        })();
        if result.is_err() {
            let _ = fs::remove_file(&temporary);
        }
        result
    }

    fn install_from_connection(&mut self, source: &Connection) -> AppResult<()> {
        let temporary_path = self.temporary_database_path();
        let result = (|| -> AppResult<()> {
            let mut destination = Connection::open(&temporary_path)?;
            Database::configure(&destination)?;
            copy_all_pages_atomically(source, &mut destination)?;
            validate_current_database(&destination, "recovery_backup")?;
            drop(destination);
            prepare_database_file_for_rename(&temporary_path)?;
            self.commit_temporary_database(&temporary_path)
        })();
        if result.is_err() {
            remove_temporary_database(&temporary_path);
        }
        result
    }

    fn commit_temporary_database(&mut self, temporary_path: &Path) -> AppResult<()> {
        if self.database.path().exists() {
            return Err(AppError::new(
                "recovery_active_exists",
                "La ubicación activa volvió a contener datos y no se sobrescribirá.",
            ));
        }
        fs::rename(temporary_path, self.database.path())?;
        self.database.set_available(true);
        if let Err(error) = self.database.initialize() {
            self.database.set_available(false);
            return Err(AppError::new(
                "recovery_commit_failed",
                format!(
                    "La base recuperada no pudo activarse. El archivo en cuarentena se conserva. {}",
                    error.message
                ),
            ));
        }
        self.issue = None;
        self.quarantine_failure = None;
        Ok(())
    }

    fn ensure_action_available(&self) -> AppResult<()> {
        if !self.is_required() {
            return Err(recovery_not_required());
        }
        if self.quarantine.is_none() || !active_database_files_absent(self.database.path()) {
            return Err(AppError::new(
                "recovery_quarantine_incomplete",
                "Vindexa no puede reemplazar datos hasta aislar la base anterior por completo.",
            ));
        }
        Ok(())
    }

    fn temporary_database_path(&self) -> PathBuf {
        self.data_directory
            .join(format!("vindexa-recovery-{}.sqlite3", Uuid::new_v4()))
    }
}

fn quarantine_database(active_path: &Path, data_directory: &Path) -> AppResult<QuarantineRecord> {
    quarantine_database_with_sidecar_mover(active_path, data_directory, move_sqlite_sidecars)
}

fn quarantine_database_with_sidecar_mover<F>(
    active_path: &Path,
    data_directory: &Path,
    move_sidecars: F,
) -> AppResult<QuarantineRecord>
where
    F: FnOnce(&Path, &Path) -> AppResult<usize>,
{
    if !active_path.is_file() {
        return Err(AppError::new(
            "recovery_quarantine",
            "La base dañada no es un archivo regular y se ha bloqueado cualquier sustitución.",
        ));
    }
    let id = Uuid::new_v4().to_string();
    let detected_at = Utc::now().to_rfc3339();
    let directory = data_directory
        .join("recovery")
        .join(format!("quarantine-{id}"));
    fs::create_dir_all(&directory)?;
    preflight_sqlite_sidecars(active_path, &directory)?;
    let database_path = directory.join("vindexa.sqlite3");
    fs::rename(active_path, &database_path)?;
    let sidecar_count = match move_sidecars(active_path, &directory) {
        Ok(count) => count,
        Err(sidecar_error) => {
            if fs::rename(&database_path, active_path).is_err() {
                return Err(AppError::new(
                    "recovery_quarantine_rollback",
                    "Falló el aislamiento de archivos auxiliares y la base principal no pudo volver a su ubicación. La recuperación permanece bloqueada.",
                ));
            }
            return Err(sidecar_error);
        }
    };
    let size_bytes = fs::metadata(&database_path)
        .map(|value| value.len())
        .unwrap_or(0);
    let (integrity, schema_version) = quarantine_diagnostics(&database_path);
    Ok(QuarantineRecord {
        summary: QuarantinedDatabaseSummary {
            id,
            detected_at,
            file_name: "vindexa.sqlite3".into(),
            size_bytes,
            sidecar_count,
            integrity,
            schema_version,
        },
        database_path,
    })
}

fn preflight_sqlite_sidecars(active_path: &Path, directory: &Path) -> AppResult<()> {
    for suffix in ["-wal", "-shm", "-journal"] {
        let source = PathBuf::from(format!("{}{suffix}", active_path.display()));
        if !source.exists() {
            continue;
        }
        let metadata = fs::symlink_metadata(&source)?;
        let destination = directory.join(format!("vindexa.sqlite3{suffix}"));
        if !metadata.is_file() || metadata.file_type().is_symlink() || destination.exists() {
            return Err(AppError::new(
                "recovery_quarantine_sidecar",
                "Un archivo auxiliar de SQLite no pudo aislarse de forma segura.",
            ));
        }
    }
    Ok(())
}

fn move_sqlite_sidecars(active_path: &Path, directory: &Path) -> AppResult<usize> {
    preflight_sqlite_sidecars(active_path, directory)?;
    let mut moved = Vec::new();
    for suffix in ["-wal", "-shm", "-journal"] {
        let source = PathBuf::from(format!("{}{suffix}", active_path.display()));
        if !source.exists() {
            continue;
        }
        let destination = directory.join(format!("vindexa.sqlite3{suffix}"));
        if fs::rename(&source, &destination).is_err() {
            for (original, quarantined) in moved.iter().rev() {
                let _ = fs::rename(quarantined, original);
            }
            return Err(AppError::new(
                "recovery_quarantine_sidecar",
                "Un archivo auxiliar de SQLite no pudo aislarse; la recuperación sigue bloqueada.",
            ));
        }
        moved.push((source, destination));
    }
    Ok(moved.len())
}

fn quarantine_diagnostics(path: &Path) -> (String, Option<i64>) {
    let Ok(connection) = open_read_only_database(path) else {
        return ("No se pudo abrir como SQLite".into(), None);
    };
    let schema_version = connection
        .pragma_query_value(None, "user_version", |row| row.get(0))
        .ok();
    let integrity = connection
        .query_row("PRAGMA integrity_check", [], |row| row.get::<_, String>(0))
        .unwrap_or_else(|_| "No se pudo completar la comprobación".into());
    (integrity, schema_version)
}

fn discover_safety_candidates(data_directory: &Path) -> Vec<RecoveryCandidate> {
    let Ok(entries) = fs::read_dir(data_directory) else {
        return Vec::new();
    };
    let mut candidates = entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| {
                    (name.starts_with("vindexa-before-restore-")
                        || name.starts_with("vindexa-backup-"))
                        && (name.ends_with(".sqlite3") || name.ends_with(".db"))
                })
        })
        .filter_map(|path| inspect_candidate(path, "safety").ok())
        .collect::<Vec<_>>();
    candidates.sort_by(|left, right| right.summary.modified_at.cmp(&left.summary.modified_at));
    candidates
}

fn inspect_candidate(path: PathBuf, source: &str) -> AppResult<RecoveryCandidate> {
    reject_unsafe_candidate_path(&path)?;
    let metadata = fs::metadata(&path)?;
    let validation = open_read_only_database(&path)
        .and_then(|connection| validate_current_database(&connection, "recovery_backup"));
    let (valid, validation_message) = match validation {
        Ok(()) => (
            true,
            "Identidad, esquema, relaciones e integridad verificados.".into(),
        ),
        Err(error) => (false, error.message),
    };
    let label = if source == "safety" {
        "Copia de seguridad automática".to_string()
    } else {
        "Copia seleccionada".to_string()
    };
    Ok(RecoveryCandidate {
        summary: RecoveryBackupSummary {
            id: Uuid::new_v4().to_string(),
            label,
            size_bytes: metadata.len(),
            modified_at: metadata.modified().ok().map(system_time_to_rfc3339),
            source: source.into(),
            valid,
            validation_message,
        },
        path,
    })
}

fn reject_unsafe_candidate_path(path: &Path) -> AppResult<()> {
    let metadata = fs::symlink_metadata(path)
        .map_err(|_| AppError::validation("La copia seleccionada ya no está disponible."))?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(AppError::validation(
            "La copia debe ser un archivo regular, nunca un enlace simbólico.",
        ));
    }
    Ok(())
}

fn prepare_database_file_for_rename(path: &Path) -> AppResult<()> {
    let connection = Connection::open(path)?;
    connection.execute_batch("PRAGMA wal_checkpoint(TRUNCATE); PRAGMA journal_mode = DELETE;")?;
    validate_current_database(&connection, "recovery_staged")?;
    drop(connection);
    Ok(())
}

fn remove_temporary_database(path: &Path) {
    for candidate in [
        path.to_path_buf(),
        PathBuf::from(format!("{}-wal", path.display())),
        PathBuf::from(format!("{}-shm", path.display())),
        PathBuf::from(format!("{}-journal", path.display())),
    ] {
        let _ = fs::remove_file(candidate);
    }
}

fn active_database_files_absent(path: &Path) -> bool {
    ![
        path.to_path_buf(),
        PathBuf::from(format!("{}-wal", path.display())),
        PathBuf::from(format!("{}-shm", path.display())),
        PathBuf::from(format!("{}-journal", path.display())),
    ]
    .iter()
    .any(|candidate| candidate.exists())
}

fn system_time_to_rfc3339(value: SystemTime) -> String {
    DateTime::<Utc>::from(value).to_rfc3339()
}

fn recovery_not_required() -> AppError {
    AppError::new(
        "recovery_not_required",
        "La base local ya está disponible y no necesita recuperación.",
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Database;
    use rusqlite::params;
    use std::fs;
    use tempfile::TempDir;

    fn initialized_database(directory: &TempDir, name: &str) -> Database {
        let database = Database::new(directory.path().join(name));
        database.initialize().expect("inicializar base");
        database
    }

    #[test]
    fn corrupted_startup_is_quarantined_without_creating_a_replacement() {
        let directory = TempDir::new().expect("crear temporal");
        let active_path = directory.path().join("vindexa.sqlite3");
        fs::write(&active_path, b"esto no es sqlite").expect("crear base corrupta");
        let database = Database::new(active_path.clone());

        let recovery = StartupRecovery::prepare(database.clone());
        let snapshot = recovery.snapshot();

        assert!(snapshot.required);
        assert!(snapshot.quarantine.is_some());
        assert!(
            !active_path.exists(),
            "el arranque no debe crear una base limpia"
        );
        assert!(
            database.open().is_err(),
            "la base permanece bloqueada hasta decidir"
        );
    }

    #[test]
    fn newer_incompatible_schema_is_quarantined_instead_of_downgraded() {
        let directory = TempDir::new().expect("crear temporal");
        let active = initialized_database(&directory, "vindexa.sqlite3");
        active
            .open()
            .expect("abrir base")
            .pragma_update(
                None,
                "user_version",
                crate::db::migrations::CURRENT_VERSION + 1,
            )
            .expect("simular esquema futuro");
        let active_path = active.path().to_path_buf();

        let recovery = StartupRecovery::prepare(active.clone());
        let snapshot = recovery.snapshot();

        assert!(snapshot.required);
        assert_eq!(
            snapshot.issue.as_ref().map(|issue| issue.code.as_str()),
            Some("database_version")
        );
        assert_eq!(
            snapshot.quarantine.and_then(|item| item.schema_version),
            Some(crate::db::migrations::CURRENT_VERSION + 1)
        );
        assert!(!active_path.exists());
        assert!(!active.is_available());
    }

    #[test]
    fn restore_requires_confirmation_and_only_accepts_a_validated_opaque_candidate() {
        let directory = TempDir::new().expect("crear temporal");
        let active_path = directory.path().join("vindexa.sqlite3");
        fs::write(&active_path, b"corrupta").expect("crear base corrupta");
        let database = Database::new(active_path);
        let mut recovery = StartupRecovery::prepare(database.clone());
        let backup = initialized_database(&directory, "vindexa-before-restore-valid.sqlite3");
        backup
            .open()
            .expect("abrir copia")
            .execute(
                "INSERT INTO games(app_id, title) VALUES (?1, ?2)",
                params![42, "Recuperado"],
            )
            .expect("insertar dato");
        recovery.refresh_candidates();
        let candidate = recovery
            .snapshot()
            .backups
            .into_iter()
            .find(|candidate| candidate.valid)
            .expect("detectar copia válida");

        let error = recovery
            .restore(&candidate.id, "")
            .expect_err("exigir confirmación");
        assert_eq!(error.code, "recovery_confirmation");
        assert!(!database.is_available());

        recovery
            .restore(&candidate.id, "RESTAURAR")
            .expect("restaurar copia verificada");
        assert!(database.is_available());
        assert_eq!(
            database
                .open()
                .expect("abrir restaurada")
                .query_row("SELECT title FROM games WHERE app_id = 42", [], |row| {
                    row.get::<_, String>(0)
                })
                .expect("leer dato"),
            "Recuperado"
        );
    }

    #[test]
    fn clean_recovery_keeps_the_quarantine_and_requires_explicit_confirmation() {
        let directory = TempDir::new().expect("crear temporal");
        let active_path = directory.path().join("vindexa.sqlite3");
        fs::write(&active_path, b"corrupta").expect("crear base corrupta");
        let database = Database::new(active_path);
        let mut recovery = StartupRecovery::prepare(database.clone());
        let quarantine_id = recovery.snapshot().quarantine.expect("cuarentena").id;

        assert!(recovery.create_clean("NO").is_err());
        recovery
            .create_clean("CREAR NUEVA")
            .expect("crear base limpia");

        assert!(database.is_available());
        assert!(!recovery.snapshot().required);
        assert!(recovery.quarantine_exists(&quarantine_id));
    }

    #[test]
    fn invalid_safety_file_is_reported_but_cannot_be_restored() {
        let directory = TempDir::new().expect("crear temporal");
        let active_path = directory.path().join("vindexa.sqlite3");
        fs::write(&active_path, b"corrupta").expect("crear base corrupta");
        fs::write(
            directory
                .path()
                .join("vindexa-before-restore-invalid.sqlite3"),
            b"tampoco sqlite",
        )
        .expect("crear falsa copia");
        let database = Database::new(active_path);
        let mut recovery = StartupRecovery::prepare(database);
        recovery.refresh_candidates();
        let candidate = recovery
            .snapshot()
            .backups
            .into_iter()
            .find(|candidate| !candidate.valid)
            .expect("mostrar copia inválida");

        let error = recovery
            .restore(&candidate.id, "RESTAURAR")
            .expect_err("rechazar copia inválida");
        assert_eq!(error.code, "recovery_backup_invalid");
    }

    #[test]
    fn quarantine_refuses_to_continue_when_any_sqlite_sidecar_cannot_move() {
        let directory = TempDir::new().expect("crear temporal");
        let active = directory.path().join("vindexa.sqlite3");
        let quarantine = directory.path().join("quarantine");
        fs::create_dir_all(&quarantine).expect("crear cuarentena");
        fs::write(format!("{}-wal", active.display()), b"wal pendiente").expect("crear WAL");
        fs::create_dir(quarantine.join("vindexa.sqlite3-wal")).expect("bloquear destino WAL");

        let error = move_sqlite_sidecars(&active, &quarantine)
            .expect_err("un sidecar residual debe bloquear la recuperación");

        assert_eq!(error.code, "recovery_quarantine_sidecar");
        assert!(PathBuf::from(format!("{}-wal", active.display())).exists());
    }

    #[test]
    fn quarantine_restores_the_main_database_when_sidecars_fail_after_its_rename() {
        let directory = TempDir::new().expect("crear temporal");
        let active = directory.path().join("vindexa.sqlite3");
        fs::write(&active, b"base que no puede quedar oculta").expect("crear base");

        let error = quarantine_database_with_sidecar_mover(
            &active,
            directory.path(),
            |active_after_rename, _| {
                assert!(
                    !active_after_rename.exists(),
                    "la base principal ya se movió"
                );
                Err(AppError::new(
                    "recovery_quarantine_sidecar",
                    "fallo simulado después del rename principal",
                ))
            },
        )
        .expect_err("propagar fallo del sidecar");

        assert_eq!(error.code, "recovery_quarantine_sidecar");
        assert_eq!(
            fs::read(&active).expect("la base principal vuelve a la ruta activa"),
            b"base que no puede quedar oculta"
        );
    }

    #[cfg(unix)]
    #[test]
    fn quarantine_export_rejects_hardlink_alias_without_truncating_the_only_copy() {
        let directory = TempDir::new().expect("crear temporal");
        let active_path = directory.path().join("vindexa.sqlite3");
        fs::write(&active_path, b"datos corruptos importantes").expect("crear base corrupta");
        let database = Database::new(active_path);
        let recovery = StartupRecovery::prepare(database);
        let source = recovery.quarantine_source().expect("archivo aislado");
        let alias = directory.path().join("alias.sqlite3");
        fs::hard_link(source, &alias).expect("crear alias duro");

        let error = recovery
            .export_quarantined_database(&alias)
            .expect_err("rechazar alias del mismo inode");

        assert_eq!(error.code, "recovery_export_alias");
        assert_eq!(
            fs::read(source).expect("leer cuarentena"),
            b"datos corruptos importantes"
        );
    }
}
