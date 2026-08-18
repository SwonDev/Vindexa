import type { LibraryScope } from "@/features/library/LibrarySidebar";
import type { ExtraFilters } from "@/features/library/LibraryToolbar";
import type { LibraryGrouping } from "@/features/library/library-grouping";
import type {
  FamilyCatalogAvailability,
  FamilyCatalogSort,
  GameSort,
  LibraryView,
} from "@/lib/types";

export interface LibrarySessionState {
  scope: LibraryScope;
  query: string;
  sort: GameSort;
  randomSeed: number;
  view: LibraryView;
  /** Corte de la lista en grupos con encabezado. */
  grouping: LibraryGrouping;
  familyAvailability: FamilyCatalogAvailability;
  familySort: FamilyCatalogSort;
  filters: ExtraFilters;
}

const initialState: LibrarySessionState = {
  scope: { kind: "all", label: "Todos los juegos" },
  query: "",
  sort: "manual",
  randomSeed: 0,
  view: "grid",
  grouping: "none",
  familyAvailability: "all",
  familySort: "availability",
  filters: {},
};

const STORAGE_KEY = "vindexa:library-session:v1";

let sessionState = initialState;
const scrollOffsets = new Map<string, number>();
const expandedSections = new Map<string, boolean>();

function sanitizeState(value: Partial<LibrarySessionState>): LibrarySessionState {
  const scope =
    value.scope && typeof value.scope.kind === "string"
      ? { ...initialState.scope, ...value.scope }
      : initialState.scope;
  return {
    ...initialState,
    ...value,
    scope: { ...scope },
    filters:
      value.filters && typeof value.filters === "object" && !Array.isArray(value.filters)
        ? { ...value.filters }
        : {},
  };
}

function loadPersisted(): void {
  if (typeof window === "undefined") return;
  try {
    const raw = window.localStorage.getItem(STORAGE_KEY);
    if (!raw) return;
    const parsed = JSON.parse(raw) as {
      state?: Partial<LibrarySessionState>;
      scroll?: Record<string, number>;
      expanded?: Record<string, boolean>;
    };
    if (parsed.state && typeof parsed.state === "object") {
      sessionState = sanitizeState(parsed.state);
    }
    for (const [key, value] of Object.entries(parsed.scroll ?? {})) {
      if (typeof value === "number" && Number.isFinite(value) && value >= 0) {
        scrollOffsets.set(key, Math.round(value));
      }
    }
    for (const [key, value] of Object.entries(parsed.expanded ?? {})) {
      expandedSections.set(key, value !== false);
    }
  } catch {
    // Almacenamiento corrupto o no disponible: se arranca con la sesión limpia.
  }
}

function persist(): void {
  if (typeof window === "undefined") return;
  try {
    window.localStorage.setItem(
      STORAGE_KEY,
      JSON.stringify({
        state: sessionState,
        scroll: Object.fromEntries(scrollOffsets),
        expanded: Object.fromEntries(expandedSections),
      }),
    );
  } catch {
    // Sin cuota o modo privado: la sesión simplemente no persiste.
  }
}

loadPersisted();

export function readLibrarySession(): LibrarySessionState {
  return {
    ...sessionState,
    scope: { ...sessionState.scope },
    filters: { ...sessionState.filters },
  };
}

export function writeLibrarySession(next: LibrarySessionState): void {
  sessionState = {
    ...next,
    scope: { ...next.scope },
    filters: { ...next.filters },
  };
  persist();
}

export function libraryScopeKey(scope: LibraryScope): string {
  return `${scope.kind}:${scope.id ?? "all"}`;
}

export function readLibraryScroll(scope: LibraryScope): number {
  return scrollOffsets.get(libraryScopeKey(scope)) ?? 0;
}

export function writeLibraryScroll(scope: LibraryScope, offset: number): void {
  scrollOffsets.set(libraryScopeKey(scope), Math.max(0, Math.round(offset)));
  persist();
}

export function readLibrarySectionExpanded(section: string): boolean {
  return expandedSections.get(section) ?? true;
}

export function writeLibrarySectionExpanded(section: string, expanded: boolean): void {
  expandedSections.set(section, expanded);
  persist();
}

export function resetLibrarySessionForTests(): void {
  sessionState = initialState;
  scrollOffsets.clear();
  expandedSections.clear();
  if (typeof window !== "undefined") {
    window.localStorage?.removeItem(STORAGE_KEY);
  }
}
