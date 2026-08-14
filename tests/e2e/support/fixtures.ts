import { test as base, expect } from "@playwright/test";
import { VindexaPage } from "../pages/vindexa.page";
import { installTauriIpcHarness } from "./tauri-ipc-harness";
import { createTestState, type VindexaScenario } from "./test-data";

interface VindexaFixtures {
  scenario: VindexaScenario;
  app: VindexaPage;
}

export const test = base.extend<VindexaFixtures>({
  scenario: ["library", { option: true }],
  page: async ({ page, scenario }, use) => {
    await page.addInitScript({
      content: `(${installTauriIpcHarness.toString()})(${JSON.stringify(createTestState(scenario))});`,
    });
    await use(page);
  },
  app: async ({ page }, use) => {
    const consoleErrors: string[] = [];
    const pageErrors: string[] = [];
    page.on("console", (message) => {
      if (message.type() === "error") consoleErrors.push(message.text());
    });
    page.on("pageerror", (error) => pageErrors.push(error.message));
    await use(new VindexaPage(page));
    expect(pageErrors, "La aplicación lanzó excepciones no controladas").toEqual([]);
    expect(consoleErrors, "La consola contiene errores inesperados").toEqual([]);
  },
});

export { expect };
