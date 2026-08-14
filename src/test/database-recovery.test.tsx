import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { DatabaseRecoveryGate } from "@/features/recovery/DatabaseRecoveryGate";
import { api } from "@/lib/tauri";

vi.mock("@/lib/tauri", () => ({
  api: {
    databaseRecoveryStatus: vi.fn(),
    selectDatabaseRecoveryBackup: vi.fn(),
    refreshDatabaseRecoveryBackups: vi.fn(),
    restoreDatabaseRecoveryBackup: vi.fn(),
    createCleanDatabaseAfterRecovery: vi.fn(),
    exportQuarantinedDatabase: vi.fn(),
  },
  getErrorMessage: (error: unknown) =>
    error instanceof Error ? error.message : "No se pudo completar la recuperación.",
}));

const mockedApi = api as unknown as Record<string, ReturnType<typeof vi.fn>>;

function renderGate() {
  const client = new QueryClient({
    defaultOptions: { queries: { retry: false }, mutations: { retry: false } },
  });
  return render(
    <QueryClientProvider client={client}>
      <DatabaseRecoveryGate>
        <p>Aplicación disponible</p>
      </DatabaseRecoveryGate>
    </QueryClientProvider>,
  );
}

const recoveryStatus = {
  required: true,
  issue: { code: "database_integrity", message: "SQLite detectó daños." },
  quarantine: {
    id: "opaque-quarantine-id",
    detectedAt: "2026-08-14T12:00:00Z",
    fileName: "vindexa.sqlite3",
    sizeBytes: 4096,
    sidecarCount: 1,
    integrity: "database disk image is malformed",
    schemaVersion: 13,
  },
  backups: [
    {
      id: "opaque-backup-id",
      label: "Copia de seguridad automática",
      sizeBytes: 8192,
      modifiedAt: "2026-08-14T11:00:00Z",
      source: "safety",
      valid: true,
      validationMessage: "Identidad, esquema, relaciones e integridad verificados.",
    },
  ],
  recoveryActionsAvailable: true,
};

describe("recuperación segura de la base al arrancar", () => {
  beforeEach(() => vi.clearAllMocks());

  it("bloquea la aplicación y muestra cuarentena y copias verificadas", async () => {
    mockedApi.databaseRecoveryStatus.mockResolvedValue(recoveryStatus);
    renderGate();

    expect(
      await screen.findByRole("heading", { name: "Recuperación de datos necesaria" }),
    ).toBeVisible();
    expect(screen.queryByText("Aplicación disponible")).not.toBeInTheDocument();
    expect(screen.getByText("SQLite detectó daños.")).toBeVisible();
    expect(screen.getByText("Copia verificada")).toBeVisible();
    expect(screen.getByRole("button", { name: "Restaurar esta copia" })).toBeEnabled();
  });

  it("no restaura hasta que el usuario escribe la confirmación exacta", async () => {
    const user = userEvent.setup();
    mockedApi.databaseRecoveryStatus.mockResolvedValue(recoveryStatus);
    mockedApi.restoreDatabaseRecoveryBackup.mockResolvedValue({
      ...recoveryStatus,
      required: false,
    });
    renderGate();

    await user.click(await screen.findByRole("button", { name: "Restaurar esta copia" }));
    expect(mockedApi.restoreDatabaseRecoveryBackup).not.toHaveBeenCalled();
    await user.type(screen.getByLabelText("Confirmación de restauración"), "RESTAURAR");
    await user.click(screen.getByRole("button", { name: "Confirmar restauración" }));

    expect(mockedApi.restoreDatabaseRecoveryBackup).toHaveBeenCalledWith(
      "opaque-backup-id",
      "RESTAURAR",
    );
    expect(await screen.findByText("Aplicación disponible")).toBeVisible();
  });

  it("separa la creación limpia y conserva explícitamente la cuarentena", async () => {
    const user = userEvent.setup();
    mockedApi.databaseRecoveryStatus.mockResolvedValue({ ...recoveryStatus, backups: [] });
    mockedApi.createCleanDatabaseAfterRecovery.mockResolvedValue({
      ...recoveryStatus,
      required: false,
    });
    renderGate();

    await user.click(await screen.findByRole("button", { name: "Crear una base vacía" }));
    expect(screen.getByText(/El archivo en cuarentena no se eliminará/)).toBeVisible();
    await user.type(screen.getByLabelText("Confirmación de base nueva"), "CREAR NUEVA");
    await user.click(screen.getByRole("button", { name: "Confirmar base nueva" }));

    expect(mockedApi.createCleanDatabaseAfterRecovery).toHaveBeenCalledWith("CREAR NUEVA");
  });
});
