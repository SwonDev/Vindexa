import {
  IconAlertTriangle,
  IconDatabase,
  IconDownload,
  IconFileCheck,
  IconFolderOpen,
  IconRefresh,
  IconShieldLock,
} from "@tabler/icons-react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { type ReactNode, useState } from "react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { api, getErrorMessage } from "@/lib/tauri";
import type { DatabaseRecoverySnapshot, RecoveryBackupSummary } from "@/lib/types";

interface DatabaseRecoveryGateProps {
  children: ReactNode;
}

type ConfirmationMode =
  | { kind: "restore"; candidate: RecoveryBackupSummary }
  | { kind: "clean" }
  | undefined;

export function DatabaseRecoveryGate({ children }: DatabaseRecoveryGateProps) {
  const queryClient = useQueryClient();
  const [confirmationMode, setConfirmationMode] = useState<ConfirmationMode>();
  const [confirmation, setConfirmation] = useState("");
  const [notice, setNotice] = useState<string>();
  const recoveryQuery = useQuery({
    queryKey: ["database-recovery"],
    queryFn: api.databaseRecoveryStatus,
    staleTime: Number.POSITIVE_INFINITY,
    retry: false,
  });

  const acceptSnapshot = (snapshot: DatabaseRecoverySnapshot) => {
    queryClient.setQueryData(["database-recovery"], snapshot);
    setConfirmationMode(undefined);
    setConfirmation("");
    setNotice(undefined);
    if (!snapshot.required) {
      void queryClient.invalidateQueries({ queryKey: ["bootstrap"] });
    }
  };

  const selectBackup = useMutation({
    mutationFn: api.selectDatabaseRecoveryBackup,
    onSuccess: acceptSnapshot,
    onError: (error) => setNotice(getErrorMessage(error)),
  });
  const refreshBackups = useMutation({
    mutationFn: api.refreshDatabaseRecoveryBackups,
    onSuccess: acceptSnapshot,
    onError: (error) => setNotice(getErrorMessage(error)),
  });
  const restoreBackup = useMutation({
    mutationFn: ({ candidateId, phrase }: { candidateId: string; phrase: string }) =>
      api.restoreDatabaseRecoveryBackup(candidateId, phrase),
    onSuccess: acceptSnapshot,
    onError: (error) => setNotice(getErrorMessage(error)),
  });
  const createClean = useMutation({
    mutationFn: (phrase: string) => api.createCleanDatabaseAfterRecovery(phrase),
    onSuccess: acceptSnapshot,
    onError: (error) => setNotice(getErrorMessage(error)),
  });
  const exportQuarantine = useMutation({
    mutationFn: api.exportQuarantinedDatabase,
    onSuccess: (exported) => {
      if (exported)
        setNotice("Copia de diagnóstico guardada. La cuarentena original sigue intacta.");
    },
    onError: (error) => setNotice(getErrorMessage(error)),
  });

  if (recoveryQuery.isPending) {
    return (
      <main className="database-recovery database-recovery--loading">
        <span className="loading-state__mark" aria-hidden="true" />
        <p role="status">Comprobando la integridad de tus datos…</p>
      </main>
    );
  }
  if (recoveryQuery.isError || !recoveryQuery.data) {
    return (
      <main className="database-recovery database-recovery--loading">
        <IconAlertTriangle aria-hidden="true" size={30} />
        <h1>No se pudo comprobar la base local</h1>
        <p>{getErrorMessage(recoveryQuery.error)}</p>
        <Button onClick={() => recoveryQuery.refetch()}>
          <IconRefresh aria-hidden="true" /> Reintentar comprobación
        </Button>
      </main>
    );
  }
  const recovery = recoveryQuery.data;
  if (!recovery.required) return children;

  const busy =
    selectBackup.isPending ||
    refreshBackups.isPending ||
    restoreBackup.isPending ||
    createClean.isPending ||
    exportQuarantine.isPending;
  const expectedPhrase = confirmationMode?.kind === "restore" ? "RESTAURAR" : "CREAR NUEVA";

  return (
    <main className="database-recovery">
      <section className="database-recovery__shell" aria-labelledby="database-recovery-title">
        <header className="database-recovery__header">
          <span className="database-recovery__shield" aria-hidden="true">
            <IconShieldLock size={28} />
          </span>
          <div>
            <p className="eyebrow">Protección local de Vindexa</p>
            <h1 id="database-recovery-title">Recuperación de datos necesaria</h1>
            <p>
              El arranque se ha detenido antes de escribir datos nuevos. Decide cómo continuar con
              la base anterior aislada.
            </p>
          </div>
        </header>

        {recovery.issue && (
          <div className="database-recovery__issue" role="alert">
            <IconAlertTriangle aria-hidden="true" />
            <div>
              <strong>{recovery.issue.code}</strong>
              <p>{recovery.issue.message}</p>
            </div>
          </div>
        )}

        <div className="database-recovery__grid">
          <section className="database-recovery__panel">
            <div className="database-recovery__panel-title">
              <IconDatabase aria-hidden="true" />
              <div>
                <h2>Archivo en cuarentena</h2>
                <p>No se modifica ni se elimina durante la recuperación.</p>
              </div>
            </div>
            {recovery.quarantine ? (
              <dl className="database-recovery__facts">
                <div>
                  <dt>Tamaño</dt>
                  <dd>{formatBytes(recovery.quarantine.sizeBytes)}</dd>
                </div>
                <div>
                  <dt>Esquema detectado</dt>
                  <dd>{recovery.quarantine.schemaVersion ?? "No disponible"}</dd>
                </div>
                <div>
                  <dt>Integridad</dt>
                  <dd>{recovery.quarantine.integrity}</dd>
                </div>
                <div>
                  <dt>Archivos auxiliares</dt>
                  <dd>{recovery.quarantine.sidecarCount}</dd>
                </div>
              </dl>
            ) : (
              <p className="database-recovery__blocked">
                El aislamiento no pudo completarse. Las acciones que crean o restauran datos están
                bloqueadas para evitar una sobrescritura.
              </p>
            )}
            <Button
              type="button"
              variant="outline"
              disabled={!recovery.quarantine || busy}
              onClick={() => exportQuarantine.mutate()}
            >
              <IconDownload aria-hidden="true" /> Guardar copia para diagnóstico
            </Button>
          </section>

          <section className="database-recovery__panel">
            <div className="database-recovery__panel-title">
              <IconFileCheck aria-hidden="true" />
              <div>
                <h2>Copias disponibles</h2>
                <p>Solo una copia verificada puede sustituir la base activa.</p>
              </div>
            </div>
            <div className="database-recovery__backups">
              {recovery.backups.length === 0 ? (
                <p className="database-recovery__empty">No se encontraron copias conocidas.</p>
              ) : (
                recovery.backups.map((backup) => (
                  <article
                    className="database-recovery__backup"
                    data-valid={backup.valid}
                    key={backup.id}
                  >
                    <div>
                      <strong>{backup.label}</strong>
                      <span>
                        {backup.modifiedAt ? formatDate(backup.modifiedAt) : "Fecha no disponible"}
                      </span>
                    </div>
                    <p>{backup.validationMessage}</p>
                    <div className="database-recovery__backup-footer">
                      <span>{backup.valid ? "Copia verificada" : "Copia no válida"}</span>
                      <Button
                        type="button"
                        size="sm"
                        disabled={!backup.valid || !recovery.recoveryActionsAvailable || busy}
                        onClick={() => {
                          setConfirmationMode({ kind: "restore", candidate: backup });
                          setConfirmation("");
                          setNotice(undefined);
                        }}
                      >
                        Restaurar esta copia
                      </Button>
                    </div>
                  </article>
                ))
              )}
            </div>
            <div className="database-recovery__toolbar">
              <Button
                type="button"
                variant="secondary"
                disabled={busy}
                onClick={() => selectBackup.mutate()}
              >
                <IconFolderOpen aria-hidden="true" /> Seleccionar otra copia
              </Button>
              <Button
                type="button"
                variant="ghost"
                disabled={busy}
                onClick={() => refreshBackups.mutate()}
              >
                <IconRefresh aria-hidden="true" /> Volver a comprobar
              </Button>
            </div>
          </section>
        </div>

        <section className="database-recovery__clean">
          <div>
            <h2>Empezar con una base vacía</h2>
            <p>
              Úsalo solo si no quieres restaurar una copia. El archivo en cuarentena no se eliminará
              y podrás conservarlo para diagnóstico.
            </p>
          </div>
          <Button
            type="button"
            variant="outline"
            disabled={!recovery.recoveryActionsAvailable || busy}
            onClick={() => {
              setConfirmationMode({ kind: "clean" });
              setConfirmation("");
              setNotice(undefined);
            }}
          >
            Crear una base vacía
          </Button>
        </section>

        {confirmationMode && (
          <section className="database-recovery__confirmation" aria-live="polite">
            <div>
              <h2>
                {confirmationMode.kind === "restore"
                  ? "Confirma la restauración"
                  : "Confirma la base nueva"}
              </h2>
              <p>
                Escribe <strong>{expectedPhrase}</strong>. La cuarentena permanecerá intacta.
              </p>
            </div>
            <Input
              autoFocus
              aria-label={
                confirmationMode.kind === "restore"
                  ? "Confirmación de restauración"
                  : "Confirmación de base nueva"
              }
              autoComplete="off"
              spellCheck={false}
              value={confirmation}
              onChange={(event) => setConfirmation(event.target.value)}
            />
            <div className="database-recovery__confirmation-actions">
              <Button
                type="button"
                variant="ghost"
                disabled={busy}
                onClick={() => setConfirmationMode(undefined)}
              >
                Cancelar
              </Button>
              <Button
                type="button"
                disabled={confirmation !== expectedPhrase || busy}
                onClick={() => {
                  if (confirmationMode.kind === "restore") {
                    restoreBackup.mutate({
                      candidateId: confirmationMode.candidate.id,
                      phrase: confirmation,
                    });
                  } else {
                    createClean.mutate(confirmation);
                  }
                }}
              >
                {confirmationMode.kind === "restore"
                  ? "Confirmar restauración"
                  : "Confirmar base nueva"}
              </Button>
            </div>
          </section>
        )}
        {notice && (
          <p className="database-recovery__notice" role="status">
            {notice}
          </p>
        )}
      </section>
    </main>
  );
}

function formatBytes(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${Math.round(bytes / 1024)} KB`;
  return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
}

function formatDate(value: string): string {
  const date = new Date(value);
  return Number.isNaN(date.getTime())
    ? "Fecha no disponible"
    : new Intl.DateTimeFormat("es-ES", { dateStyle: "medium", timeStyle: "short" }).format(date);
}
