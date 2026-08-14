import type { LibraryScope } from "@/features/library/LibrarySidebar";
import type { ExtraFilters } from "@/features/library/LibraryToolbar";
import type { GameSort, LibraryView } from "@/lib/types";

export interface LibrarySessionState {
  scope: LibraryScope;
  query: string;
  sort: GameSort;
  randomSeed: number;
  view: LibraryView;
  filters: ExtraFilters;
}

const initialState: LibrarySessionState = {
  scope: { kind: "all", label: "Todos los juegos" },
  query: "",
  sort: "manual",
  randomSeed: 0,
  view: "grid",
  filters: {},
};

let sessionState = initialState;
const scrollOffsets = new Map<string, number>();
const expandedSections = new Map<string, boolean>();

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
}

export function libraryScopeKey(scope: LibraryScope): string {
  return `${scope.kind}:${scope.id ?? "all"}`;
}

export function readLibraryScroll(scope: LibraryScope): number {
  return scrollOffsets.get(libraryScopeKey(scope)) ?? 0;
}

export function writeLibraryScroll(scope: LibraryScope, offset: number): void {
  scrollOffsets.set(libraryScopeKey(scope), Math.max(0, Math.round(offset)));
}

export function readLibrarySectionExpanded(section: string): boolean {
  return expandedSections.get(section) ?? true;
}

export function writeLibrarySectionExpanded(section: string, expanded: boolean): void {
  expandedSections.set(section, expanded);
}

export function resetLibrarySessionForTests(): void {
  sessionState = initialState;
  scrollOffsets.clear();
  expandedSections.clear();
}
