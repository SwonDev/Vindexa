import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { describe, expect, it } from "vitest";

const clientPath = resolve(process.cwd(), "src/lib/tauri.ts");
const rustRegistryPath = resolve(process.cwd(), "src-tauri/src/lib.rs");

describe("registro cruzado de comandos Tauri", () => {
  it("registra en Rust cada comando invocado por el frontend", () => {
    const client = readFileSync(clientPath, "utf8");
    const registry = readFileSync(rustRegistryPath, "utf8");
    const invokedCommands = Array.from(
      client.matchAll(/invoke(?:<[^>]+>)?\("([a-z0-9_]+)"/g),
      (match) => match[1],
    ).filter((command): command is string => Boolean(command));

    expect(invokedCommands.length).toBeGreaterThanOrEqual(20);
    expect(new Set(invokedCommands).size).toBe(invokedCommands.length);

    const missing = invokedCommands.filter(
      (command) => !registry.includes(`commands::${command},`),
    );
    expect(missing, `Comandos frontend sin registro Rust: ${missing.join(", ")}`).toEqual([]);
  });
});
