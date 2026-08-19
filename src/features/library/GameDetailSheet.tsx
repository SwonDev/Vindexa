import { zodResolver } from "@hookform/resolvers/zod";
import {
  IconBrandSteam,
  IconCalendar,
  IconCheck,
  IconChevronDown,
  IconClock,
  IconDeviceGamepad2,
  IconDownload,
  IconFolderOpen,
  IconLanguage,
  IconLoader2,
  IconMoodKid,
  IconMovie,
  IconPlayerPlay,
  IconRefresh,
  IconRosetteDiscountCheck,
  IconRoute,
  IconShieldCheck,
  IconShieldLock,
  IconStarFilled,
  IconTargetArrow,
  IconTrash,
  IconWorld,
} from "@tabler/icons-react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { MotionConfig, motion, useReducedMotion } from "motion/react";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { useForm, useWatch } from "react-hook-form";
import { z } from "zod";
import { Artwork } from "@/components/common/Artwork";
import { Eyebrow } from "@/components/common/Eyebrow";
import { LoadingState } from "@/components/common/LoadingState";
import { MetricStrip } from "@/components/common/MetricStrip";
import {
  AlertDialog,
  AlertDialogAction,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
  AlertDialogTrigger,
} from "@/components/ui/alert-dialog";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Checkbox } from "@/components/ui/checkbox";
import { Input } from "@/components/ui/input";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import {
  Sheet,
  SheetContent,
  SheetDescription,
  SheetHeader,
  SheetTitle,
} from "@/components/ui/sheet";
import { Slider } from "@/components/ui/slider";
import { Switch } from "@/components/ui/switch";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { Textarea } from "@/components/ui/textarea";
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip";
import "@/features/library/game-detail.css";
import { CopyableValue } from "@/components/motion";
import { DLC_SUMMARY_STALE_MS, GameDlcPanel } from "@/features/library/GameDlcPanel";
import { PersonalJournal } from "@/features/library/PersonalJournal";
import { PriorityExplanation } from "@/features/library/PriorityExplanation";
import { GameVideoPanel } from "@/features/wishlist/GameVideoPanel";
import {
  formatBytes,
  formatDate,
  formatPlaytime,
  formatRelativeDate,
  formatSteamDeckStatus,
} from "@/lib/format";
import { storeLabel } from "@/lib/stores";
import { api, getErrorMessage } from "@/lib/tauri";
import type { CollectionSummary, GameDetail, StatusDefinition, UpdateGameInput } from "@/lib/types";

const NEXT_ACTION_MAX = 500;
const NOTES_MAX = 20_000;
const DESCRIPTION_CLAMP_CHARS = 420;
const CATEGORY_CHIPS_VISIBLE = 6;
/** Altura a la que se pliega la prosa larga; coincide con `--detail-prose-clamp`. */
const PROSE_CLAMP_PX = 248;
/** Holgura para no plegar por una línea huérfana. */
const PROSE_CLAMP_TOLERANCE_PX = 24;
const MEDIA_VISIBLE = 6;

/* ───────────────────────────────────────────────────────────────────────────
   Contrato de metadatos enriquecidos.

   El backend (`src-tauri/src/db/rich_metadata.rs`) ya define y persiste estos
   campos, pero todavía no viajan en `GameDetail` de `src/lib/types.ts`. Se
   declaran aquí como opcionales y se consumen con acceso defensivo para que la
   ficha funcione igual antes y después de que el comando los exponga. El diff
   exacto para `types.ts` va en el informe de la tarea.
   ────────────────────────────────────────────────────────────────────────── */

export type DescriptionBlock =
  | { kind: "heading"; level?: number; text: string }
  | { kind: "paragraph"; text: string }
  | { kind: "list"; ordered?: boolean; items: string[] };

export interface StructuredDescription {
  blocks?: DescriptionBlock[];
}

export type DrmState = "unknown" | "drm_free" | "third_party_drm" | "steam_drm";

export interface DrmEvidence {
  source: string;
  match: string;
}

export interface GameMediaItem {
  mediaId: string;
  kind: "screenshot" | "movie";
  thumbnailUrl?: string;
  fullUrl?: string;
  altUrl?: string;
  position?: number;
}

export interface GameDetailExtras {
  detailedDescription?: StructuredDescription | string | null;
  aboutTheGame?: StructuredDescription | string | null;
  supportedLanguages?: string | null;
  websiteUrl?: string | null;
  metacriticScore?: number | null;
  metacriticUrl?: string | null;
  requiredAge?: number | null;
  controllerSupport?: string | null;
  drmState?: DrmState | null;
  drmNotice?: string | null;
  drmEvidence?: DrmEvidence[] | null;
  /** Forma alternativa: el backend agrupa estado y evidencias en `drm`. */
  drm?: { state?: DrmState; evidence?: DrmEvidence[] } | null;
  media?: GameMediaItem[] | null;
}

type EnrichedDetail = GameDetail & Partial<GameDetailExtras>;

const detailSchema = z.object({
  appId: z.number().int().positive(),
  statusId: z.string().min(1),
  progress: z.number().int().min(0).max(100),
  priority: z.number().int().min(0).max(5),
  pinned: z.boolean(),
  tracking: z.boolean(),
  rating: z.number().int().min(1).max(10).optional(),
  estimatedMinutes: z.number().int().positive().optional(),
  targetDate: z.string().optional(),
  nextAction: z.string().max(NEXT_ACTION_MAX).optional(),
  checkpoint: z.string().max(2_000).optional(),
  notes: z.string().max(NOTES_MAX).optional(),
});

interface Props {
  appId?: number;
  open: boolean;
  onOpenChange: (open: boolean) => void;
  statuses: StatusDefinition[];
  collections: CollectionSummary[];
  confirmUninstall?: boolean;
}
interface ActionRequest {
  id: "play" | "install" | "uninstall" | "folder" | "store";
  pending: string;
  success: string;
  run: () => Promise<unknown>;
}
interface ActionNotice {
  state: "pending" | "success" | "error";
  message: string;
}

interface MetadataRefreshRequest {
  gameId: number;
  force: boolean;
}

interface AutosaveJob {
  fingerprint: string;
  values: UpdateGameInput;
}

export function GameDetailSheet({
  appId,
  open,
  onOpenChange,
  statuses,
  collections,
  confirmUninstall = true,
}: Props) {
  const queryClient = useQueryClient();
  const prefersReducedMotion = useReducedMotion();
  const initializedId = useRef<number | undefined>(undefined);
  const metadataAttemptedId = useRef<number | undefined>(undefined);
  const scrollRef = useRef<HTMLDivElement | null>(null);
  const heroMediaRef = useRef<HTMLDivElement | null>(null);
  const [actionNotice, setActionNotice] = useState<ActionNotice>();
  const [achievementError, setAchievementError] = useState<string>();
  const [invalidDraft, setInvalidDraft] = useState(false);
  const [descExpanded, setDescExpanded] = useState(false);
  const [categoriesExpanded, setCategoriesExpanded] = useState(false);
  const [mediaExpanded, setMediaExpanded] = useState(false);
  const [failedMedia, setFailedMedia] = useState<readonly string[]>([]);
  const [proseNode, setProseNode] = useState<HTMLDivElement | null>(null);
  const [proseOverflows, setProseOverflows] = useState<boolean | undefined>(undefined);
  const detailQuery = useQuery({
    queryKey: ["game", appId],
    queryFn: () => api.gameDetail(appId as number),
    enabled: open && Boolean(appId),
  });
  // Sólo alimenta el contador de la pestaña. `GameDlcPanel` comparte esta misma
  // clave, así que abrir la pestaña no vuelve a pedir el resumen.
  const dlcSummaryQuery = useQuery({
    queryKey: ["game-dlc-summary", appId],
    queryFn: () => api.dlcSummary(appId as number),
    enabled: open && Boolean(appId),
    staleTime: DLC_SUMMARY_STALE_MS,
  });
  const form = useForm<z.infer<typeof detailSchema>>({
    resolver: zodResolver(detailSchema),
    defaultValues: {
      appId: 0,
      statusId: "unclassified",
      progress: 0,
      priority: 0,
      pinned: false,
      tracking: false,
      nextAction: "",
      checkpoint: "",
      notes: "",
    },
  });
  const watched = useWatch({ control: form.control });
  const parallaxAppId = detailQuery.data?.appId;
  const queuedSaveRef = useRef<AutosaveJob | undefined>(undefined);
  const saveLoopRef = useRef<Promise<void> | undefined>(undefined);
  const startSaveLoopRef = useRef<(() => Promise<void>) | undefined>(undefined);
  const lastEnqueuedFingerprintRef = useRef<string | undefined>(undefined);
  const lastPersistedFingerprintRef = useRef<string | undefined>(undefined);
  const mutation = useMutation({
    mutationFn: (input: UpdateGameInput) => api.updateGame(input),
    onSuccess: (detail, input) => {
      lastPersistedFingerprintRef.current = saveFingerprint(input);
      const current = detailSchema.safeParse(form.getValues());
      if (current.success && saveFingerprint(current.data) === saveFingerprint(input)) {
        form.reset(detailToForm(detail));
      }
      queryClient.setQueryData(["game", detail.appId], detail);
      void queryClient.invalidateQueries({ queryKey: ["games"] });
      void queryClient.invalidateQueries({ queryKey: ["bootstrap"] });
    },
  });
  const mutateAsyncRef = useRef(mutation.mutateAsync);
  mutateAsyncRef.current = mutation.mutateAsync;
  const drainSaveQueue = useCallback(async () => {
    while (queuedSaveRef.current) {
      const job = queuedSaveRef.current;
      queuedSaveRef.current = undefined;
      try {
        await mutateAsyncRef.current(job.values);
      } catch {
        if (lastEnqueuedFingerprintRef.current === job.fingerprint) {
          lastEnqueuedFingerprintRef.current = undefined;
        }
      }
    }
  }, []);
  const startSaveLoop = useCallback(() => {
    if (saveLoopRef.current) return saveLoopRef.current;
    const loop = drainSaveQueue().finally(() => {
      if (saveLoopRef.current === loop) saveLoopRef.current = undefined;
      if (queuedSaveRef.current) void startSaveLoopRef.current?.();
    });
    saveLoopRef.current = loop;
    return loop;
  }, [drainSaveQueue]);
  startSaveLoopRef.current = startSaveLoop;
  const queueSave = useCallback(
    (input: UpdateGameInput, force = false) => {
      const parsed = detailSchema.safeParse(input);
      if (!parsed.success) {
        setInvalidDraft(true);
        return Promise.resolve();
      }
      setInvalidDraft(false);
      const fingerprint = saveFingerprint(parsed.data);
      if (
        !force &&
        (lastEnqueuedFingerprintRef.current === fingerprint ||
          lastPersistedFingerprintRef.current === fingerprint)
      ) {
        return saveLoopRef.current ?? Promise.resolve();
      }
      lastEnqueuedFingerprintRef.current = fingerprint;
      queuedSaveRef.current = { fingerprint, values: parsed.data };
      return startSaveLoop();
    },
    [startSaveLoop],
  );
  const flushCurrentSave = useCallback(
    (force = false) => {
      const current = detailSchema.safeParse(form.getValues());
      if (current.success) {
        void queueSave(current.data, force);
      } else {
        setInvalidDraft(true);
      }
    },
    [form, queueSave],
  );
  const metadataMutation = useMutation({
    mutationFn: ({ gameId, force }: MetadataRefreshRequest) =>
      api.refreshGameMetadata(gameId, force),
    onSuccess: (detail) => queryClient.setQueryData(["game", detail.appId], detail),
  });
  const achievementMutation = useMutation({
    mutationFn: (gameId: number) => api.refreshGameAchievements(gameId),
    onMutate: () => setAchievementError(undefined),
    onSuccess: (nextDetail) => {
      setAchievementError(undefined);
      queryClient.setQueryData(["game", nextDetail.appId], nextDetail);
    },
    onError: async (error) => {
      setAchievementError(getErrorMessage(error));
      await queryClient.invalidateQueries({ queryKey: ["game", appId] });
    },
  });
  const collectionMutation = useMutation({
    mutationFn: (collectionIds: string[]) => api.setGameCollections(appId as number, collectionIds),
    onSuccess: (detail) => {
      queryClient.setQueryData(["game", appId], detail);
      void queryClient.invalidateQueries({ queryKey: ["bootstrap"] });
    },
  });
  const actionMutation = useMutation({
    mutationFn: async (request: ActionRequest) => {
      await request.run();
      return request.success;
    },
    onMutate: (request) => setActionNotice({ state: "pending", message: request.pending }),
    onSuccess: (message) => setActionNotice({ state: "success", message }),
    onError: (error) => setActionNotice({ state: "error", message: getErrorMessage(error) }),
  });

  useEffect(() => {
    if (!detailQuery.data || initializedId.current === detailQuery.data.appId) return;
    initializedId.current = detailQuery.data.appId;
    const values = detailToForm(detailQuery.data);
    lastPersistedFingerprintRef.current = saveFingerprint(values);
    setInvalidDraft(false);
    setDescExpanded(false);
    setCategoriesExpanded(false);
    setMediaExpanded(false);
    setFailedMedia([]);
    form.reset(values);
  }, [detailQuery.data, form]);
  useEffect(() => {
    if (open) return;
    initializedId.current = undefined;
    metadataAttemptedId.current = undefined;
    setActionNotice(undefined);
    setAchievementError(undefined);
    setInvalidDraft(false);
  }, [open]);
  useEffect(() => {
    return () => {
      flushCurrentSave();
    };
  }, [flushCurrentSave]);
  useEffect(() => {
    const detail = detailQuery.data;
    if (!open || !detail || metadataAttemptedId.current === detail.appId) return;
    metadataAttemptedId.current = detail.appId;
    metadataMutation.mutate({ gameId: detail.appId, force: false });
  }, [detailQuery.data, metadataMutation, open]);
  useEffect(() => {
    if (!watched.appId || !form.formState.isDirty || !appId) return;
    const timer = window.setTimeout(() => flushCurrentSave(), 700);
    return () => window.clearTimeout(timer);
  }, [appId, flushCurrentSave, form.formState.isDirty, watched]);
  useEffect(() => {
    const scroller = scrollRef.current;
    const media = heroMediaRef.current;
    if (!open || !parallaxAppId || !scroller || !media) return;
    const reduced = window.matchMedia("(prefers-reduced-motion: reduce)");
    let frame: number | undefined;
    const paint = () => {
      frame = undefined;
      if (reduced.matches) {
        media.style.transform = "none";
        return;
      }
      // Sólo `transform`: el desvanecido anterior apagaba el banner sobre el
      // fondo del panel y era una de las causas del color lavado.
      const scroll = Math.min(scroller.scrollTop, 320);
      media.style.transform = `translate3d(0, ${Math.round(scroll * 0.18)}px, 0) scale(1.06)`;
    };
    const schedule = () => {
      if (frame === undefined) frame = window.requestAnimationFrame(paint);
    };
    paint();
    scroller.addEventListener("scroll", schedule, { passive: true });
    reduced.addEventListener("change", schedule);
    return () => {
      scroller.removeEventListener("scroll", schedule);
      reduced.removeEventListener("change", schedule);
      if (frame !== undefined) window.cancelAnimationFrame(frame);
      media.style.transform = "";
    };
  }, [open, parallaxAppId]);
  // La prosa se pliega por altura REAL medida. Cuando el navegador todavía no
  // ha medido (primer pintado, jsdom) se usa la estimación por longitud, así el
  // botón nunca aparece y desaparece provocando un salto de maquetación.
  useEffect(() => {
    if (!proseNode || typeof ResizeObserver === "undefined") {
      setProseOverflows(undefined);
      return;
    }
    const measure = () => {
      const height = proseNode.getBoundingClientRect().height;
      setProseOverflows(
        height > 0 ? height > PROSE_CLAMP_PX + PROSE_CLAMP_TOLERANCE_PX : undefined,
      );
    };
    measure();
    const observer = new ResizeObserver(measure);
    observer.observe(proseNode);
    return () => observer.disconnect();
  }, [proseNode]);

  const detail = detailQuery.data as EnrichedDetail | undefined;
  const longBlocks = useMemo(
    () => toDescriptionBlocks(detail?.detailedDescription ?? detail?.aboutTheGame),
    [detail?.aboutTheGame, detail?.detailedDescription],
  );
  const specs = useMemo(() => (detail ? readSpecs(detail) : []), [detail]);
  const media = useMemo(
    () => readMedia(detail).filter((item) => !failedMedia.includes(item.mediaId)),
    [detail, failedMedia],
  );
  // Una captura que el CDN no sirve no puede dejar un hueco roto en la rejilla.
  const dropMedia = useCallback((mediaId: string) => {
    setFailedMedia((failed) => (failed.includes(mediaId) ? failed : [...failed, mediaId]));
  }, []);
  const proseLength =
    (detail?.shortDescription?.length ?? 0) +
    longBlocks.reduce((total, block) => total + blockLength(block), 0);
  const hasProse = Boolean(detail?.shortDescription) || longBlocks.length > 0;
  const needsClamp = proseOverflows ?? proseLength > DESCRIPTION_CLAMP_CHARS;
  const proseCollapsed = hasProse && needsClamp && !descExpanded;
  const submitAction = (request: ActionRequest) => {
    if (!actionMutation.isPending) actionMutation.mutate(request);
  };
  const uninstallRequest = detailQuery.data
    ? {
        id: "uninstall" as const,
        pending: "Entregando la solicitud a Steam…",
        success:
          "Steam recibió la solicitud de desinstalación. Revisa su ventana para confirmarla y completar el proceso.",
        run: () => api.uninstallGame(detailQuery.data?.appId as number),
      }
    : undefined;
  return (
    <Sheet
      open={open}
      onOpenChange={(nextOpen) => {
        if (!nextOpen) flushCurrentSave();
        onOpenChange(nextOpen);
      }}
    >
      <SheetContent ref={scrollRef} className="game-detail-sheet" side="right">
        {detailQuery.isPending ? (
          <LoadingState label="Cargando ficha" />
        ) : detailQuery.isError ? (
          <div className="detail-error">
            <strong>No se pudo abrir la ficha</strong>
            <p>{getErrorMessage(detailQuery.error)}</p>
            <Button onClick={() => detailQuery.refetch()}>Reintentar</Button>
          </div>
        ) : detail ? (
          <MotionConfig reducedMotion="user">
            <motion.div
              key={`hero-${detail.appId}`}
              className="detail-hero"
              initial={prefersReducedMotion ? false : { opacity: 0.82, y: -8 }}
              animate={{ opacity: 1, y: 0 }}
              transition={{ duration: 0.22, ease: [0.16, 1, 0.3, 1] }}
            >
              <div className="detail-hero__media" ref={heroMediaRef}>
                <Artwork
                  appId={detail.appId}
                  src={detail.heroUrl ?? detail.headerUrl ?? detail.coverUrl}
                  title={detail.title}
                  kind={detail.heroUrl ? "hero" : "header"}
                  priority
                />
              </div>
              <div className="detail-hero__scrim" />
              <div className="detail-hero__badges">
                {detail.installed && <Badge variant="secondary">INSTALADO LOCALMENTE</Badge>}
                {detail.isFree && <Badge variant="secondary">FREE TO PLAY</Badge>}
                {detail.ownershipSource === "family_shared" && (
                  <Badge variant="outline">BIBLIOTECA FAMILIAR</Badge>
                )}
                <Badge variant="outline" style={{ borderColor: detail.statusColor }}>
                  {detail.statusName}
                </Badge>
              </div>
              <SheetHeader>
                {/* Un juego de Epic, GOG o itch.io no tiene AppID: el número
                    que lleva se lo inventa Vindexa para poder organizarlo, y
                    enseñarlo como «APP de Steam» era afirmar algo falso. Ahí se
                    nombra la tienda; el AppID sólo aparece cuando es de verdad,
                    y entonces se puede copiar de un clic. */}
                {detail.externalStore ? (
                  <Eyebrow>{storeLabel(detail.externalStore).toUpperCase()}</Eyebrow>
                ) : (
                  <Eyebrow>
                    <span className="detail-appid">
                      STEAM · APP{" "}
                      <CopyableValue
                        value={String(detail.appId)}
                        label={`Copiar el AppID ${detail.appId}`}
                      />
                    </span>
                  </Eyebrow>
                )}
                <SheetTitle>{detail.title}</SheetTitle>
                <SheetDescription>
                  {[detail.developer, detail.releaseDate && formatDate(detail.releaseDate)]
                    .filter(Boolean)
                    .join(" · ") || detail.statusName}
                </SheetDescription>
              </SheetHeader>
            </motion.div>
            <div className="detail-actions">
              <Button
                size="sm"
                disabled={actionMutation.isPending}
                onClick={() =>
                  submitAction({
                    id: detail.installed ? "play" : "install",
                    pending: detail.installed ? "Abriendo el juego…" : "Abriendo la instalación…",
                    success: detail.installed
                      ? "Steam recibió la orden de iniciar el juego."
                      : "Steam recibió la orden de instalar el juego.",
                    run: () =>
                      detail.installed
                        ? api.launchGame(detail.appId)
                        : api.installGame(detail.appId),
                  })
                }
              >
                {actionMutation.isPending &&
                ["play", "install"].includes(actionMutation.variables?.id ?? "") ? (
                  <IconLoader2 className="is-spinning" />
                ) : detail.installed ? (
                  <IconPlayerPlay />
                ) : (
                  <IconDownload />
                )}
                {detail.installed ? "Jugar" : "Instalar"}
              </Button>
              {detail.installed && (
                <Button
                  size="sm"
                  variant="secondary"
                  disabled={actionMutation.isPending}
                  onClick={() =>
                    submitAction({
                      id: "folder",
                      pending: "Localizando la instalación…",
                      success: "La instalación se abrió en el explorador de archivos.",
                      run: () => api.revealInstallation(detail.appId),
                    })
                  }
                >
                  <IconFolderOpen /> Mostrar instalación
                </Button>
              )}
              {detail.installed &&
                uninstallRequest &&
                (confirmUninstall ? (
                  <AlertDialog>
                    <AlertDialogTrigger asChild>
                      <Button
                        size="sm"
                        variant="ghost"
                        className="detail-action--destructive"
                        disabled={actionMutation.isPending}
                      >
                        <IconTrash /> Desinstalar
                      </Button>
                    </AlertDialogTrigger>
                    <AlertDialogContent>
                      <AlertDialogHeader>
                        <AlertDialogTitle>
                          ¿Solicitar la desinstalación de {detail.title}?
                        </AlertDialogTitle>
                        <AlertDialogDescription>
                          Vindexa no borrará archivos directamente. Abrirá el cliente oficial de
                          Steam para que revise y complete la desinstalación.
                        </AlertDialogDescription>
                      </AlertDialogHeader>
                      <AlertDialogFooter>
                        <AlertDialogCancel>Cancelar</AlertDialogCancel>
                        <AlertDialogAction
                          variant="destructive"
                          onClick={() => submitAction(uninstallRequest)}
                        >
                          Solicitar a Steam
                        </AlertDialogAction>
                      </AlertDialogFooter>
                    </AlertDialogContent>
                  </AlertDialog>
                ) : (
                  <Button
                    size="sm"
                    variant="ghost"
                    className="detail-action--destructive"
                    disabled={actionMutation.isPending}
                    onClick={() => submitAction(uninstallRequest)}
                  >
                    <IconTrash /> Desinstalar
                  </Button>
                ))}
              <Button
                size="sm"
                variant="ghost"
                disabled={actionMutation.isPending}
                onClick={() =>
                  submitAction({
                    id: "store",
                    pending: "Abriendo la tienda protegida…",
                    success: "La tienda oficial se abrió en una sesión privada de Vindexa.",
                    run: () => api.openStore(detail.appId),
                  })
                }
              >
                <IconBrandSteam /> Tienda integrada
              </Button>
              <span
                className="autosave-state"
                data-error={mutation.isError || invalidDraft || undefined}
                aria-live="polite"
              >
                {mutation.isPending ? (
                  <>
                    <IconLoader2 className="is-spinning" /> Guardando
                  </>
                ) : invalidDraft ? (
                  "Sin guardar: revisa los campos marcados"
                ) : mutation.isError ? (
                  getErrorMessage(mutation.error)
                ) : form.formState.isDirty ? (
                  "Cambios pendientes"
                ) : (
                  <>
                    <IconCheck /> Guardado
                  </>
                )}
              </span>
            </div>
            {actionNotice && (
              <div
                className="detail-action-notice"
                data-state={actionNotice.state}
                role={actionNotice.state === "error" ? "alert" : "status"}
                aria-live={actionNotice.state === "error" ? "assertive" : "polite"}
              >
                {actionNotice.state === "pending" ? (
                  <IconLoader2 className="is-spinning" />
                ) : actionNotice.state === "success" ? (
                  <IconCheck />
                ) : (
                  <IconRefresh />
                )}
                <span>{actionNotice.message}</span>
                {actionNotice.state === "error" && actionMutation.variables && (
                  <Button
                    size="xs"
                    variant="secondary"
                    onClick={() => submitAction(actionMutation.variables as ActionRequest)}
                  >
                    Reintentar
                  </Button>
                )}
              </div>
            )}
            <MetricStrip
              className="detail-metrics"
              label="Resumen de la partida"
              items={[
                {
                  id: "playtime",
                  label: "Tiempo de juego",
                  value: formatPlaytime(detail.playtimeMinutes),
                },
                {
                  id: "recent",
                  label: "Reciente (2 sem)",
                  value: formatPlaytime(detail.playtimeRecentMinutes),
                },
                {
                  id: "last-session",
                  label: "Última sesión",
                  value: formatDate(detail.lastPlayedAt),
                },
                {
                  id: "achievements",
                  label: "Logros",
                  icon: <IconRosetteDiscountCheck size={13} />,
                  value:
                    detail.achievementsStatus === "success" &&
                    typeof detail.achievementsUnlocked === "number" &&
                    typeof detail.achievementsTotal === "number"
                      ? `${detail.achievementsUnlocked}/${detail.achievementsTotal}`
                      : "Sin datos",
                  note: detail.achievementsFetchedAt
                    ? `Act. ${formatRelativeDate(detail.achievementsFetchedAt)}`
                    : undefined,
                  action:
                    detail.achievementsStatus !== "success" ? (
                      <Button
                        size="xs"
                        variant="ghost"
                        disabled={achievementMutation.isPending}
                        onClick={() => achievementMutation.mutate(detail.appId)}
                      >
                        {achievementMutation.isPending ? (
                          <IconLoader2 className="is-spinning" />
                        ) : (
                          <IconRefresh />
                        )}
                        {achievementMutation.isPending ? "Actualizando…" : "Actualizar logros"}
                      </Button>
                    ) : undefined,
                },
                {
                  id: "steam-deck",
                  label: "Steam Deck",
                  icon: <IconDeviceGamepad2 size={13} />,
                  value: formatSteamDeckStatus(detail.steamDeckStatus) ?? "Sin datos",
                },
                { id: "size", label: "En disco", value: formatBytes(detail.sizeOnDisk) },
              ]}
            />
            {achievementError && (
              <div className="detail-achievement-feedback" role="alert" aria-live="assertive">
                <IconRosetteDiscountCheck />
                <span>{achievementError}</span>
                <Button
                  size="xs"
                  variant="secondary"
                  disabled={achievementMutation.isPending}
                  onClick={() => achievementMutation.mutate(detail.appId)}
                >
                  Reintentar logros
                </Button>
              </div>
            )}
            <motion.section
              key={`overview-${detail.appId}`}
              className="detail-overview detail-about"
              aria-labelledby="detail-about-title"
              initial={prefersReducedMotion ? false : { opacity: 0.88, y: 6 }}
              animate={{ opacity: 1, y: 0 }}
              transition={{ duration: 0.18, delay: 0.05, ease: [0.16, 1, 0.3, 1] }}
            >
              <div className="detail-about__copy">
                {/* El título ya preside el banner: repetirlo aquí es relleno de
                    plantilla. El encabezado accesible se conserva fuera de la
                    vista para que la sección siga teniendo nombre. */}
                <div className="detail-about__head">
                  <h3 id="detail-about-title" className="sr-only">
                    Acerca de {detail.title}
                  </h3>
                  <Eyebrow decorative>ACERCA DE ESTE JUEGO</Eyebrow>
                </div>
                {hasProse ? (
                  <>
                    <div
                      className="detail-about__prose"
                      id="detail-description"
                      data-collapsed={proseCollapsed || undefined}
                    >
                      <div ref={setProseNode}>
                        {detail.shortDescription ? (
                          <p className="detail-about__lead">{detail.shortDescription}</p>
                        ) : null}
                        <DescriptionBlocks blocks={longBlocks} />
                      </div>
                    </div>
                    {needsClamp && (
                      <Button
                        size="xs"
                        variant="ghost"
                        className="detail-about__toggle"
                        aria-expanded={descExpanded}
                        aria-controls="detail-description"
                        onClick={() => setDescExpanded((expanded) => !expanded)}
                      >
                        <IconChevronDown />
                        {descExpanded ? "Mostrar menos" : "Mostrar más"}
                      </Button>
                    )}
                  </>
                ) : metadataMutation.isPending || detail.metadataStatus === "pending" ? (
                  <p className="detail-about__state">
                    <IconLoader2 className="is-spinning" /> Cargando la descripción desde Steam…
                  </p>
                ) : (
                  <p className="detail-about__state">
                    La descripción de Steam no está disponible ahora. Tu organización personal y los
                    datos locales siguen accesibles.
                  </p>
                )}
              </div>
              <div className="detail-overview__tags detail-about__tags">
                {detail.isEarlyAccess && <Badge variant="outline">Early Access</Badge>}
                {detail.genres.slice(0, 6).map((genre) => (
                  <Badge key={genre} variant="secondary">
                    {genre}
                  </Badge>
                ))}
                {detail.metadataStatus === "failed" && (
                  <Badge variant="destructive">Ficha de Steam sin conexión</Badge>
                )}
                {detail.metadataStatus === "unavailable" && (
                  <Badge variant="outline">Ficha no publicada</Badge>
                )}
                {["failed", "unavailable"].includes(detail.metadataStatus) && (
                  <Button
                    size="xs"
                    variant="secondary"
                    disabled={metadataMutation.isPending}
                    onClick={() => metadataMutation.mutate({ gameId: detail.appId, force: true })}
                  >
                    {metadataMutation.isPending ? (
                      <IconLoader2 className="is-spinning" />
                    ) : (
                      <IconRefresh />
                    )}
                    Reintentar ficha
                  </Button>
                )}
              </div>
            </motion.section>
            {specs.length > 0 && (
              <section className="detail-specs" aria-labelledby="detail-specs-title">
                <h3 id="detail-specs-title">Especificaciones</h3>
                <dl className="detail-specs__list">
                  {specs.map((spec) => (
                    <div
                      className={spec.wide ? "detail-spec detail-spec--wide" : "detail-spec"}
                      key={spec.id}
                    >
                      <dt>
                        {spec.icon}
                        {spec.label}
                      </dt>
                      <dd>{spec.value}</dd>
                    </div>
                  ))}
                </dl>
              </section>
            )}
            {media.length > 0 && (
              <section className="detail-media" aria-labelledby="detail-media-title">
                <h3 id="detail-media-title">Capturas y vídeos</h3>
                <ul className="detail-media__grid">
                  {(mediaExpanded ? media : media.slice(0, MEDIA_VISIBLE)).map((item, index) => (
                    <li key={item.mediaId}>
                      {item.kind === "movie" ? (
                        <button
                          type="button"
                          className="detail-media__item"
                          onClick={() =>
                            submitAction({
                              id: "store",
                              pending: "Abriendo la tienda protegida…",
                              success:
                                "La tienda oficial se abrió en una sesión privada de Vindexa.",
                              run: () => api.openStore(detail.appId),
                            })
                          }
                        >
                          <img
                            src={item.thumbnailUrl}
                            alt={`Vídeo ${index + 1} de ${detail.title}. Se abre en la tienda integrada.`}
                            loading="lazy"
                            decoding="async"
                            onError={() => dropMedia(item.mediaId)}
                          />
                          <span className="detail-media__badge">
                            <IconMovie /> VÍDEO
                          </span>
                        </button>
                      ) : (
                        <span className="detail-media__item">
                          <img
                            src={item.thumbnailUrl}
                            alt={`Captura ${index + 1} de ${detail.title}`}
                            loading="lazy"
                            decoding="async"
                            onError={() => dropMedia(item.mediaId)}
                          />
                        </span>
                      )}
                    </li>
                  ))}
                </ul>
                {media.length > MEDIA_VISIBLE && (
                  <Button
                    size="xs"
                    variant="ghost"
                    className="detail-media__more"
                    aria-expanded={mediaExpanded}
                    onClick={() => setMediaExpanded((expanded) => !expanded)}
                  >
                    {mediaExpanded
                      ? "Mostrar menos medios"
                      : `Mostrar ${media.length - MEDIA_VISIBLE} medios más`}
                  </Button>
                )}
              </section>
            )}
            <Tabs defaultValue="plan" className="detail-tabs">
              <TabsList>
                <TabsTrigger value="plan">Plan personal</TabsTrigger>
                <TabsTrigger value="info">Información</TabsTrigger>
                <TabsTrigger value="dlc">
                  Contenido adicional
                  {dlcSummaryQuery.data && dlcSummaryQuery.data.total > 0 ? (
                    <span className="detail-tab-count">{dlcSummaryQuery.data.total}</span>
                  ) : null}
                </TabsTrigger>
                <TabsTrigger value="videos">Vídeos</TabsTrigger>
                <TabsTrigger value="journal">Registro</TabsTrigger>
                <TabsTrigger value="history">Actividad</TabsTrigger>
              </TabsList>
              <TabsContent value="plan">
                <PriorityExplanation key={`priority-${detail.appId}`} appId={detail.appId} />
                <DetailForm
                  detail={detail}
                  statuses={statuses}
                  collections={collections}
                  watched={watched}
                  form={form}
                  onSave={(values, force) => void queueSave(values, force)}
                  savePending={mutation.isPending}
                  collectionMutation={collectionMutation}
                />
              </TabsContent>
              <TabsContent value="info" className="detail-info">
                <section className="detail-info__group" aria-labelledby="detail-info-steam-title">
                  <h3 id="detail-info-steam-title">Steam</h3>
                  <InfoRow label="Desarrollador" value={detail.developer} />
                  <InfoRow label="Editor" value={detail.publisher} />
                  <InfoRow label="Lanzamiento" value={displayDate(detail.releaseDate)} />
                  <InfoRow label="Modelo" value={detail.isFree ? "Free to Play" : "De pago"} />
                  <InfoRow label="Géneros" value={detail.genres.join(", ")} />
                  {detail.categories.length > 0 && (
                    <div className="info-row">
                      <span>Categorías</span>
                      <div className="info-chips" id="detail-info-categories">
                        {(categoriesExpanded
                          ? detail.categories
                          : detail.categories.slice(0, CATEGORY_CHIPS_VISIBLE)
                        ).map((category) => (
                          <Badge key={category} variant="secondary">
                            {category}
                          </Badge>
                        ))}
                        {detail.categories.length > CATEGORY_CHIPS_VISIBLE && (
                          <Button
                            size="xs"
                            variant="ghost"
                            aria-expanded={categoriesExpanded}
                            aria-controls="detail-info-categories"
                            onClick={() => setCategoriesExpanded((expanded) => !expanded)}
                          >
                            {categoriesExpanded
                              ? "Mostrar menos"
                              : `Mostrar ${detail.categories.length - CATEGORY_CHIPS_VISIBLE} más`}
                          </Button>
                        )}
                      </div>
                    </div>
                  )}
                </section>
                <section className="detail-info__group" aria-labelledby="detail-info-copy-title">
                  <h3 id="detail-info-copy-title">Tu copia</h3>
                  <InfoRow label="Propiedad" value={ownershipLabel(detail.ownershipSource)} />
                  {detail.ownershipSource === "family_shared" && (
                    <InfoRow
                      label="Disponibilidad familiar"
                      value={familyAvailabilityLabel(detail.familyAvailability)}
                    />
                  )}
                  <InfoRow label="Instalación" value={detail.installPath} />
                </section>
                <section className="detail-info__group" aria-labelledby="detail-info-sync-title">
                  <h3 id="detail-info-sync-title">Sincronización</h3>
                  {detail.metadataFetchedAt ? (
                    <InfoRow
                      label="Ficha actualizada"
                      value={formatDate(detail.metadataFetchedAt)}
                    />
                  ) : (
                    <p className="muted-copy">
                      Todavía no se ha descargado la ficha oficial de Steam.
                    </p>
                  )}
                </section>
              </TabsContent>
              <TabsContent value="dlc">
                <GameDlcPanel
                  key={`dlc-${detail.appId}`}
                  appId={detail.appId}
                  title={detail.title}
                />
              </TabsContent>
              <TabsContent value="videos">
                {/* El mismo panel que usa Deseados. La dirección del `iframe`
                    la construye Rust y apunta a `youtube-nocookie.com`, el
                    único origen que la política de contenido de la ventana
                    admite en un marco: reproduce sin el seguimiento habitual de
                    YouTube y sin que la página pueda llevarte a ningún otro
                    sitio. */}
                <GameVideoPanel
                  key={`videos-${detail.appId}`}
                  appId={detail.appId}
                  title={detail.title}
                  headerUrl={detail.headerUrl}
                  coverUrl={detail.coverUrl}
                />
              </TabsContent>
              <TabsContent value="journal">
                <PersonalJournal detail={detail} />
              </TabsContent>
              <TabsContent value="history" className="timeline">
                {detail.activity.length ? (
                  detail.activity.map((item) => (
                    <div className="timeline-item" key={item.id}>
                      <i />
                      <div>
                        <strong>{item.message}</strong>
                        <span>{formatDate(item.createdAt)}</span>
                      </div>
                    </div>
                  ))
                ) : (
                  <p className="muted-copy">
                    La actividad personal aparecerá aquí cuando cambies estados, progreso o notas.
                  </p>
                )}
              </TabsContent>
            </Tabs>
          </MotionConfig>
        ) : null}
      </SheetContent>
    </Sheet>
  );
}

type DetailValues = z.infer<typeof detailSchema>;
function DetailForm({
  detail,
  statuses,
  collections,
  watched,
  form,
  onSave,
  savePending,
  collectionMutation,
}: {
  detail: GameDetail;
  statuses: StatusDefinition[];
  collections: CollectionSummary[];
  watched: Partial<DetailValues>;
  form: ReturnType<typeof useForm<DetailValues>>;
  onSave: (values: UpdateGameInput, force?: boolean) => void;
  savePending: boolean;
  collectionMutation: ReturnType<typeof useMutation<GameDetail, Error, string[]>>;
}) {
  const nextActionLength = watched.nextAction?.length ?? 0;
  const notesLength = watched.notes?.length ?? 0;
  const estimatedMinutesValue = watched.estimatedMinutes ?? 0;
  const nextActionError =
    nextActionLength > NEXT_ACTION_MAX
      ? `La próxima acción supera el máximo de ${NEXT_ACTION_MAX} caracteres.`
      : undefined;
  const notesError =
    notesLength > NOTES_MAX ? "Las notas superan el máximo de 20 000 caracteres." : undefined;
  return (
    <form className="detail-form" onSubmit={form.handleSubmit((values) => onSave(values, true))}>
      <fieldset className="detail-form__group">
        <legend>Estado y progreso</legend>
        <div className="detail-form__row">
          <div className="detail-field">
            <span>Estado</span>
            <Select
              value={watched.statusId ?? "unclassified"}
              onValueChange={(value) => {
                if (!value) return;
                form.setValue("statusId", value, { shouldDirty: true });
              }}
            >
              <SelectTrigger aria-label="Estado personal">
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                {statuses.map((status) => (
                  <SelectItem key={status.id} value={status.id}>
                    <span className="select-status">
                      <i style={{ backgroundColor: status.color }} />
                      {status.name}
                    </span>
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
          </div>
          <div className="detail-field">
            <span>Valoración</span>
            <Select
              value={watched.rating ? String(watched.rating) : "none"}
              onValueChange={(value) => {
                if (!value) return;
                form.setValue("rating", value === "none" ? undefined : Number(value), {
                  shouldDirty: true,
                });
              }}
            >
              <SelectTrigger aria-label="Valoración personal">
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                <SelectItem value="none">Sin valorar</SelectItem>
                {Array.from({ length: 10 }, (_, index) => index + 1).map((rating) => (
                  <SelectItem key={rating} value={String(rating)}>
                    {rating}/10
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
          </div>
        </div>
        <div className="slider-field">
          <span>
            <span>Progreso</span>
            <output>{watched.progress ?? 0}%</output>
          </span>
          <Slider
            aria-label="Progreso del juego"
            value={[watched.progress ?? 0]}
            min={0}
            max={100}
            step={1}
            onValueChange={([progress = 0]) =>
              form.setValue("progress", progress, { shouldDirty: true })
            }
          />
        </div>
        <div className="detail-toggles">
          <div className="detail-toggle">
            <Switch
              aria-label="Fijar en la biblioteca"
              checked={watched.pinned ?? false}
              onCheckedChange={(pinned) => form.setValue("pinned", pinned, { shouldDirty: true })}
            />
            <span>Fijado en la biblioteca</span>
          </div>
        </div>
      </fieldset>
      <fieldset className="detail-form__group">
        <legend>Planificación</legend>
        <div className="slider-field">
          <span>
            <span>Prioridad</span>
            <output>{watched.priority ?? 0}/5</output>
          </span>
          <Slider
            aria-label="Prioridad del juego"
            value={[watched.priority ?? 0]}
            min={0}
            max={5}
            step={1}
            onValueChange={([priority = 0]) =>
              form.setValue("priority", priority, { shouldDirty: true })
            }
          />
          <p className="detail-form__hint">De 0 (sin prisa) a 5 (lo próximo que quieres jugar).</p>
        </div>
        <div className="detail-form__row">
          <label htmlFor="game-target-date">
            <span>
              <IconCalendar size={14} /> Fecha objetivo
            </span>
            <Input id="game-target-date" type="date" {...form.register("targetDate")} />
          </label>
          <label htmlFor="game-estimated-minutes">
            <span>
              <IconClock size={14} /> Duración restante
            </span>
            {/* La unidad viaja con el campo: un «540» a secas no dice nada, y
                bajo el control se muestra además convertido a horas. */}
            <span className="detail-form__measure">
              <Input
                id="game-estimated-minutes"
                type="number"
                min={1}
                step={15}
                placeholder="Minutos"
                aria-describedby="game-estimated-minutes-hint"
                {...form.register("estimatedMinutes", {
                  setValueAs: (value) => (value === "" ? undefined : Number(value)),
                })}
              />
              <em>min</em>
            </span>
            <small id="game-estimated-minutes-hint" className="detail-form__hint">
              {estimatedMinutesValue
                ? `Equivale a ${formatPlaytime(estimatedMinutesValue)}.`
                : "En minutos. Se usa para calcular la capacidad del planificador."}
            </small>
          </label>
        </div>
        <label htmlFor="game-checkpoint">
          <span>
            <IconRoute size={14} /> ¿Por dónde lo dejaste?
          </span>
          <Textarea
            id="game-checkpoint"
            rows={3}
            placeholder="Checkpoint, misión, zona o situación actual"
            {...form.register("checkpoint")}
          />
          <p className="detail-form__hint">
            Una pista breve para retomar semanas después sin releer tus notas.
          </p>
        </label>
        <div className="detail-field">
          <label htmlFor="game-next-action">
            <span>
              <IconTargetArrow size={14} /> Próxima acción
            </span>
            <Input
              id="game-next-action"
              maxLength={NEXT_ACTION_MAX}
              placeholder="La siguiente cosa concreta que quieres hacer"
              aria-invalid={Boolean(nextActionError)}
              aria-describedby="game-next-action-meta"
              {...form.register("nextAction")}
            />
          </label>
          <FieldMeta
            id="game-next-action-meta"
            length={nextActionLength}
            max={NEXT_ACTION_MAX}
            error={nextActionError}
          />
        </div>
      </fieldset>
      <fieldset className="detail-form__group">
        <legend>Notas y seguimiento</legend>
        <div className="detail-field">
          <label htmlFor="game-notes">
            <span>Notas privadas</span>
            <Textarea
              id="game-notes"
              rows={5}
              maxLength={NOTES_MAX}
              placeholder="Decisiones, estrategia o contexto que quieras conservar"
              aria-invalid={Boolean(notesError)}
              aria-describedby="game-notes-meta"
              {...form.register("notes")}
            />
          </label>
          <FieldMeta id="game-notes-meta" length={notesLength} max={NOTES_MAX} error={notesError} />
        </div>
        <div className="detail-toggles">
          <div className="detail-toggle">
            <Switch
              aria-label="Seguir actualizaciones"
              checked={watched.tracking ?? false}
              onCheckedChange={(tracking) =>
                form.setValue("tracking", tracking, { shouldDirty: true })
              }
            />
            <span>Seguir actualizaciones</span>
          </div>
        </div>
      </fieldset>
      {collections.some((collection) => collection.kind === "manual") && (
        <fieldset className="detail-collections">
          <legend>Colecciones manuales</legend>
          {collections
            .filter((collection) => collection.kind === "manual")
            .map((collection) => {
              const checked = detail.collectionIds.includes(collection.id);
              return (
                <div className="detail-collection" key={collection.id}>
                  <Checkbox
                    aria-label={`Incluir en ${collection.name}`}
                    checked={checked}
                    disabled={collectionMutation.isPending}
                    onCheckedChange={(next) =>
                      collectionMutation.mutate(
                        next === true
                          ? [...detail.collectionIds, collection.id]
                          : detail.collectionIds.filter((id) => id !== collection.id),
                      )
                    }
                  />
                  <i style={{ backgroundColor: collection.color }} />
                  <span>{collection.name}</span>
                </div>
              );
            })}
        </fieldset>
      )}
      <Button type="submit" size="sm" disabled={!form.formState.isDirty || savePending}>
        Guardar ahora
      </Button>
    </form>
  );
}

function detailToForm(detail: GameDetail): DetailValues {
  return {
    appId: detail.appId,
    statusId: detail.statusId,
    progress: detail.progress,
    priority: detail.priority,
    pinned: detail.pinned,
    tracking: detail.tracking,
    rating: detail.rating,
    estimatedMinutes: detail.estimatedMinutes,
    targetDate: detail.targetDate ?? "",
    nextAction: detail.nextAction ?? "",
    checkpoint: detail.checkpoint ?? "",
    notes: detail.notes ?? "",
  };
}

function saveFingerprint(input: UpdateGameInput): string {
  return JSON.stringify(input);
}
// Un campo ausente no se dibuja: nada de filas con «—» ocupando espacio.
function InfoRow({ label, value }: { label: string; value: string | undefined }) {
  if (!value) return null;
  return (
    <div className="info-row">
      <span>{label}</span>
      <strong>{value}</strong>
    </div>
  );
}

function FieldMeta({
  id,
  length,
  max,
  error,
}: {
  id: string;
  length: number;
  max: number;
  error?: string | undefined;
}) {
  return (
    <span className="detail-field__meta" id={id}>
      {error ? (
        <span className="field-error" role="alert">
          {error}
        </span>
      ) : null}
      <span className="field-counter" data-limit-reached={length >= max || undefined}>
        {length}/{max}
      </span>
    </span>
  );
}

function displayDate(value?: string): string | undefined {
  return value ? formatDate(value) : undefined;
}

/* ───────────────────────────────────────────────────────────────────────────
   Descripciones estructuradas.

   El backend entrega bloques ya saneados (`heading`, `paragraph`, `list`), de
   modo que la ficha nunca necesita `dangerouslySetInnerHTML`. Si en su lugar
   llegase texto plano, se divide por líneas en blanco y se maqueta igual.
   ────────────────────────────────────────────────────────────────────────── */

function toDescriptionBlocks(
  value: StructuredDescription | string | null | undefined,
): DescriptionBlock[] {
  if (!value) return [];
  if (typeof value === "string") {
    return value
      .split(/\n{2,}/)
      .map((paragraph) => paragraph.trim())
      .filter(Boolean)
      .map((text) => ({ kind: "paragraph", text }) as DescriptionBlock);
  }
  if (!Array.isArray(value.blocks)) return [];
  return value.blocks.filter((block): block is DescriptionBlock => {
    if (!block || typeof block !== "object") return false;
    if (block.kind === "list") return Array.isArray(block.items) && block.items.length > 0;
    return typeof block.text === "string" && block.text.trim().length > 0;
  });
}

function blockLength(block: DescriptionBlock): number {
  if (block.kind === "list") return block.items.join(" ").length;
  return block.text.length;
}

// Los bloques de la tienda no traen identificador propio. En vez de usar el
// índice del array (que reordena estado al cambiar la descripción) se deriva
// una clave del propio contenido, con sufijo sólo cuando hay repeticiones.
function contentKey(seen: Map<string, number>, content: string): string {
  const base = content.slice(0, 64);
  const repeated = seen.get(base) ?? 0;
  seen.set(base, repeated + 1);
  return repeated === 0 ? base : `${base}#${repeated}`;
}

function DescriptionBlocks({ blocks }: { blocks: DescriptionBlock[] }) {
  if (!blocks.length) return null;
  const seen = new Map<string, number>();
  return (
    <>
      {blocks.map((block) => {
        if (block.kind === "list") {
          const key = contentKey(seen, `list:${block.items.join("|")}`);
          const itemsSeen = new Map<string, number>();
          const items = block.items.map((item) => (
            <li key={contentKey(itemsSeen, item)}>{item}</li>
          ));
          return block.ordered ? <ol key={key}>{items}</ol> : <ul key={key}>{items}</ul>;
        }
        const key = contentKey(seen, `${block.kind}:${block.text}`);
        if (block.kind === "heading") return <h4 key={key}>{block.text}</h4>;
        return <p key={key}>{block.text}</p>;
      })}
    </>
  );
}

/* ───────────────────────────────────────────────────────────────────────────
   Especificaciones.
   ────────────────────────────────────────────────────────────────────────── */

interface SpecEntry {
  id: string;
  label: string;
  icon: React.ReactNode;
  value: React.ReactNode;
  wide?: boolean;
}

const DRM_LABEL: Record<DrmState, string> = {
  unknown: "Sin clasificar",
  drm_free: "Sin DRM",
  third_party_drm: "DRM de terceros",
  steam_drm: "Steam DRM",
};

const DRM_EXPLANATION: Record<DrmState, string> = {
  unknown: "La tienda oficial no publica señales suficientes para clasificar la protección.",
  drm_free:
    "La tienda oficial no declara DRM de terceros ni cuenta externa para este juego. La marca vive aquí, nunca sobre la carátula.",
  third_party_drm: "La tienda oficial declara un DRM o un lanzador de terceros.",
  steam_drm: "La tienda oficial declara Steamworks DRM o el propio cliente de Steam.",
};

function readDrm(detail: EnrichedDetail): { state: DrmState; evidence: DrmEvidence[] } | undefined {
  const state = detail.drmState ?? detail.drm?.state;
  if (!state || !(state in DRM_LABEL)) return undefined;
  const evidence = detail.drmEvidence ?? detail.drm?.evidence ?? [];
  return { state, evidence: Array.isArray(evidence) ? evidence : [] };
}

function splitLanguages(value: string): string[] {
  return value
    .split(/[,;]/)
    .map((language) => language.replace(/\*/g, "").trim())
    .filter(Boolean);
}

function metacriticBand(score: number): "high" | "mid" | "low" {
  if (score >= 75) return "high";
  if (score >= 50) return "mid";
  return "low";
}

function hostnameOf(url: string): string | undefined {
  try {
    return new URL(url).hostname.replace(/^www\./, "");
  } catch {
    return undefined;
  }
}

function readSpecs(detail: EnrichedDetail): SpecEntry[] {
  const specs: SpecEntry[] = [];
  const languages = detail.supportedLanguages ? splitLanguages(detail.supportedLanguages) : [];
  if (languages.length) {
    specs.push({
      id: "languages",
      label: "Idiomas",
      icon: <IconLanguage />,
      wide: languages.length > 4,
      value: (
        <span className="detail-spec__languages">
          {languages.map((language) => (
            <span className="detail-spec__language" key={language}>
              {language}
            </span>
          ))}
        </span>
      ),
    });
  }
  if (typeof detail.requiredAge === "number" && detail.requiredAge > 0) {
    specs.push({
      id: "age",
      label: "Edad recomendada",
      icon: <IconMoodKid />,
      value: `${detail.requiredAge}+`,
    });
  }
  if (detail.controllerSupport === "full" || detail.controllerSupport === "partial") {
    specs.push({
      id: "controller",
      label: "Mando",
      icon: <IconDeviceGamepad2 />,
      value: detail.controllerSupport === "full" ? "Compatible completo" : "Compatible parcial",
    });
  }
  if (typeof detail.metacriticScore === "number") {
    const host = detail.metacriticUrl ? hostnameOf(detail.metacriticUrl) : undefined;
    specs.push({
      id: "metacritic",
      label: "Metacritic",
      icon: <IconStarFilled />,
      value: (
        <>
          <span className="detail-spec__score" data-band={metacriticBand(detail.metacriticScore)}>
            {detail.metacriticScore}
            <span aria-hidden="true">/100</span>
          </span>
          {host ? <span className="muted-copy"> · {host}</span> : null}
        </>
      ),
    });
  }
  const drm = readDrm(detail);
  if (drm && (drm.state !== "unknown" || detail.drmNotice)) {
    specs.push({
      id: "drm",
      label: "Protección",
      icon: drm.state === "drm_free" ? <IconShieldCheck /> : <IconShieldLock />,
      wide: Boolean(detail.drmNotice) || drm.evidence.length > 1,
      value: <DrmValue state={drm.state} evidence={drm.evidence} notice={detail.drmNotice} />,
    });
  }
  if (detail.websiteUrl) {
    const host = hostnameOf(detail.websiteUrl);
    if (host) {
      specs.push({
        id: "website",
        label: "Sitio oficial",
        icon: <IconWorld />,
        value: host,
      });
    }
  }
  return specs;
}

function DrmValue({
  state,
  evidence,
  notice,
}: {
  state: DrmState;
  evidence: DrmEvidence[];
  notice?: string | null | undefined;
}) {
  return (
    <Tooltip>
      <TooltipTrigger asChild>
        <button type="button" className="detail-drm" data-state={state}>
          {state === "drm_free" ? <IconShieldCheck /> : <IconShieldLock />}
          {DRM_LABEL[state]}
        </button>
      </TooltipTrigger>
      <TooltipContent side="top">
        <span>
          {DRM_EXPLANATION[state]}
          {notice ? ` Aviso oficial: ${notice}` : ""}
          {evidence.length
            ? ` Señales: ${evidence.map((item) => `${item.source} → ${item.match}`).join("; ")}`
            : ""}
        </span>
      </TooltipContent>
    </Tooltip>
  );
}

/* ───────────────────────────────────────────────────────────────────────────
   Medios.
   ────────────────────────────────────────────────────────────────────────── */

function readMedia(detail: EnrichedDetail | undefined): GameMediaItem[] {
  if (!detail || !Array.isArray(detail.media)) return [];
  return detail.media
    .filter(
      (item): item is GameMediaItem =>
        Boolean(item?.mediaId) &&
        (item.kind === "screenshot" || item.kind === "movie") &&
        Boolean(item.thumbnailUrl),
    )
    .slice()
    .sort((left, right) => {
      if (left.kind !== right.kind) return left.kind === "screenshot" ? -1 : 1;
      return (left.position ?? 0) - (right.position ?? 0);
    });
}

function ownershipLabel(source: GameDetail["ownershipSource"]): string {
  if (source === "family_shared") return "Compartido por la familia";
  if (source === "local") return "Detectado localmente";
  return "En propiedad";
}

function familyAvailabilityLabel(value: GameDetail["familyAvailability"]): string {
  if (value === "confirmed") return "Disponible";
  if (value === "unknown") return "Sin confirmar";
  return "No aplicable";
}
