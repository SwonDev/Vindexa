import { tmpdir } from "node:os";
import { join } from "node:path";
import { defineConfig } from "@playwright/test";

/**
 * El puerto del servidor de pruebas.
 *
 * Fijo por omisión para que la orden sea siempre la misma, pero configurable:
 * un `vite preview` olvidado en 4173 —o cualquier otra cosa escuchando ahí—
 * dejaba la suite sin poder arrancar, y matar procesos ajenos para correr unas
 * pruebas no es una opción.
 */
const port = Number(process.env.VINDEXA_E2E_PORT ?? 4173);

export default defineConfig({
  testDir: "./tests/e2e",
  testMatch: "**/*.spec.ts",
  fullyParallel: false,
  forbidOnly: Boolean(process.env.CI),
  retries: 0,
  // Un único cliente evita que la optimización bajo demanda de un chunk lazy recargue otro test.
  workers: 1,
  timeout: 30_000,
  expect: {
    timeout: 6_000,
    toHaveScreenshot: {
      animations: "disabled",
      caret: "hide",
      maxDiffPixelRatio: 0.003,
      scale: "css",
    },
  },
  reporter: process.env.CI
    ? [
        ["line"],
        ["html", { open: "never", outputFolder: join(tmpdir(), "vindexa-playwright-report") }],
      ]
    : [
        ["list"],
        ["html", { open: "never", outputFolder: join(tmpdir(), "vindexa-playwright-report") }],
      ],
  outputDir: join(tmpdir(), "vindexa-playwright-results"),
  snapshotPathTemplate: "{testDir}/__screenshots__/{arg}{ext}",
  use: {
    baseURL: `http://127.0.0.1:${port}`,
    browserName: "chromium",
    colorScheme: "dark",
    deviceScaleFactor: 1,
    hasTouch: false,
    isMobile: false,
    locale: "es-ES",
    timezoneId: "Atlantic/Canary",
    trace: "retain-on-failure",
    viewport: { width: 1440, height: 900 },
    screenshot: "only-on-failure",
    video: "retain-on-failure",
  },
  webServer: {
    command: `pnpm exec vite --host 127.0.0.1 --port ${port}`,
    url: `http://127.0.0.1:${port}`,
    // Cada ejecución posee su servidor; reutilizar uno que otra suite está cerrando produce flakes.
    reuseExistingServer: false,
    timeout: 120_000,
    stdout: "pipe",
    stderr: "pipe",
  },
  projects: [{ name: "chromium", use: { browserName: "chromium" } }],
});
