import { expect, type Locator, type Page } from "@playwright/test";

export class VindexaPage {
  readonly page: Page;
  readonly shell: Locator;
  readonly library: Locator;

  constructor(page: Page) {
    this.page = page;
    this.shell = page.locator(".app-shell");
    this.library = page.locator(".library-main");
  }

  async goto() {
    await this.page.goto("/");
    await this.page.evaluate(() => document.fonts.ready);
  }

  async waitForShell() {
    await expect(this.shell).toBeVisible();
    await expect(
      this.page.getByRole("navigation", { name: "Secciones principales" }),
    ).toBeVisible();
  }

  async navigate(
    section: "Biblioteca" | "Planificador" | "Deseados" | "Colecciones" | "Seguimiento",
  ) {
    await this.page.getByRole("button", { name: section, exact: true }).click();
  }

  gameButton(title: string) {
    return this.page.getByRole("button", { name: new RegExp(`^${escapeRegExp(title)},`) });
  }

  async openGame(title: string) {
    await this.gameButton(title).click();
    await expect(this.page.getByRole("dialog")).toBeVisible();
    await expect(
      this.page.getByRole("dialog").getByRole("heading", { name: title, level: 2 }),
    ).toBeVisible();
  }

  async expectNoHorizontalOverflow() {
    const overflow = await this.page.evaluate(() => {
      const viewport = document.documentElement.clientWidth;
      const offenders = Array.from(document.querySelectorAll<HTMLElement>("body *"))
        .filter((element) => {
          const rect = element.getBoundingClientRect();
          return rect.width > 0 && (rect.right > viewport + 1 || rect.left < -1);
        })
        .filter((element) => {
          const style = getComputedStyle(element);
          return style.position !== "fixed" && style.position !== "absolute";
        })
        .filter((element) => {
          let ancestor = element.parentElement;
          while (ancestor && ancestor !== document.body) {
            const overflowX = getComputedStyle(ancestor).overflowX;
            if (overflowX === "hidden" || overflowX === "clip") return false;
            ancestor = ancestor.parentElement;
          }
          return true;
        })
        .slice(0, 10)
        .map((element) => ({
          tag: element.tagName.toLowerCase(),
          className: element.className,
          rect: element.getBoundingClientRect().toJSON(),
        }));
      return {
        viewport,
        documentWidth: document.documentElement.scrollWidth,
        offenders,
      };
    });
    expect(overflow.documentWidth, JSON.stringify(overflow, null, 2)).toBeLessThanOrEqual(
      overflow.viewport + 1,
    );
    expect(overflow.offenders, JSON.stringify(overflow, null, 2)).toEqual([]);
  }

  async dragGameToStatus(title: string, status: string) {
    const source = this.page.getByRole("button", { name: `Arrastrar ${title}` });
    const target = this.page.getByRole("button", {
      name: new RegExp(`^${escapeRegExp(status)}(?:$|\\s)`),
    });
    const sourceBox = await source.boundingBox();
    const targetBox = await target.boundingBox();
    expect(sourceBox).not.toBeNull();
    expect(targetBox).not.toBeNull();
    if (!sourceBox || !targetBox) return;
    await this.page.mouse.move(
      sourceBox.x + sourceBox.width / 2,
      sourceBox.y + sourceBox.height / 2,
    );
    await this.page.mouse.down();
    await this.page.mouse.move(
      sourceBox.x + sourceBox.width / 2 + 12,
      sourceBox.y + sourceBox.height / 2,
      {
        steps: 4,
      },
    );
    await this.page.mouse.move(
      targetBox.x + targetBox.width / 2,
      targetBox.y + targetBox.height / 2,
      {
        steps: 12,
      },
    );
    await this.page.mouse.up();
  }
}

function escapeRegExp(value: string): string {
  return value.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
}
