import { zodResolver } from "@hookform/resolvers/zod";
import {
  IconBrandSteam,
  IconCalendar,
  IconCheck,
  IconClock,
  IconDeviceGamepad2,
  IconDownload,
  IconFolderOpen,
  IconLoader2,
  IconPlayerPlay,
  IconRefresh,
  IconRosetteDiscountCheck,
  IconRoute,
  IconTargetArrow,
  IconTrash,
} from "@tabler/icons-react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { MotionConfig, motion, useReducedMotion } from "motion/react";
import { useEffect, useRef, useState } from "react";
import { useForm, useWatch } from "react-hook-form";
import { z } from "zod";
import { Artwork } from "@/components/common/Artwork";
import { LoadingState } from "@/components/common/LoadingState";
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
import { PersonalJournal } from "@/features/library/PersonalJournal";
import { formatBytes, formatDate, formatPlaytime } from "@/lib/format";
import { api, getErrorMessage } from "@/lib/tauri";
import type { CollectionSummary, GameDetail, StatusDefinition, UpdateGameInput } from "@/lib/types";

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
  nextAction: z.string().max(500).optional(),
  checkpoint: z.string().max(2_000).optional(),
  notes: z.string().max(20_000).optional(),
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
  const detailQuery = useQuery({
    queryKey: ["game", appId],
    queryFn: () => api.gameDetail(appId as number),
    enabled: open && Boolean(appId),
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
  const mutation = useMutation({
    mutationFn: (input: UpdateGameInput) => api.updateGame(input),
    onSuccess: (detail) => {
      form.reset(detailToForm(detail));
      queryClient.setQueryData(["game", detail.appId], detail);
      void queryClient.invalidateQueries({ queryKey: ["games"] });
      void queryClient.invalidateQueries({ queryKey: ["bootstrap"] });
    },
  });
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
    form.reset(detailToForm(detailQuery.data));
  }, [detailQuery.data, form]);
  useEffect(() => {
    if (open) return;
    initializedId.current = undefined;
    metadataAttemptedId.current = undefined;
    setActionNotice(undefined);
    setAchievementError(undefined);
  }, [open]);
  useEffect(() => {
    const detail = detailQuery.data;
    if (!open || !detail || metadataAttemptedId.current === detail.appId) return;
    metadataAttemptedId.current = detail.appId;
    metadataMutation.mutate({ gameId: detail.appId, force: false });
  }, [detailQuery.data, metadataMutation, open]);
  useEffect(() => {
    if (!watched.appId || !form.formState.isDirty || !appId || mutation.isPending) return;
    const timer = window.setTimeout(
      () => void form.handleSubmit((values) => mutation.mutate(values))(),
      700,
    );
    return () => window.clearTimeout(timer);
  }, [appId, form, mutation, watched]);
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
        media.style.opacity = "1";
        return;
      }
      const scroll = Math.min(scroller.scrollTop, 320);
      media.style.transform = `translate3d(0, ${Math.round(scroll * 0.18)}px, 0) scale(1.06)`;
      media.style.opacity = String(Math.max(0.68, 1 - scroll / 720));
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
      media.style.opacity = "";
    };
  }, [open, parallaxAppId]);

  const detail = detailQuery.data;
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
    <Sheet open={open} onOpenChange={onOpenChange}>
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
                <p className="eyebrow">STEAM · APP {detail.appId}</p>
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
                      <Button size="sm" variant="outline" disabled={actionMutation.isPending}>
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
                    variant="outline"
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
                data-error={mutation.isError || undefined}
                aria-live="polite"
              >
                {mutation.isPending ? (
                  <>
                    <IconLoader2 className="is-spinning" /> Guardando
                  </>
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
            <div className="detail-metrics">
              <DetailMetric
                label="Tiempo de juego"
                value={formatPlaytime(detail.playtimeMinutes)}
              />
              <DetailMetric label="Última sesión" value={formatDate(detail.lastPlayedAt)} />
              <DetailMetric
                label="Logros"
                icon={<IconRosetteDiscountCheck />}
                value={
                  detail.achievementsStatus === "success" &&
                  typeof detail.achievementsUnlocked === "number" &&
                  typeof detail.achievementsTotal === "number"
                    ? `${detail.achievementsUnlocked}/${detail.achievementsTotal}`
                    : "Sin datos"
                }
                action={
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
                  ) : undefined
                }
              />
              <DetailMetric
                label="Steam Deck"
                icon={<IconDeviceGamepad2 />}
                value={detail.steamDeckStatus || "Sin datos"}
              />
              <DetailMetric label="En disco" value={formatBytes(detail.sizeOnDisk)} />
            </div>
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
              className="detail-overview"
              aria-labelledby="detail-about-title"
              initial={prefersReducedMotion ? false : { opacity: 0.88, y: 6 }}
              animate={{ opacity: 1, y: 0 }}
              transition={{ duration: 0.18, delay: 0.05, ease: [0.16, 1, 0.3, 1] }}
            >
              <div className="detail-overview__copy">
                <p className="eyebrow">ACERCA DE ESTE JUEGO</p>
                <h3 id="detail-about-title">{detail.title}</h3>
                {detail.shortDescription ? (
                  <p>{detail.shortDescription}</p>
                ) : metadataMutation.isPending || detail.metadataStatus === "pending" ? (
                  <p className="detail-description-state">
                    <IconLoader2 className="is-spinning" /> Cargando la descripción desde Steam…
                  </p>
                ) : (
                  <p className="detail-description-state">
                    La descripción de Steam no está disponible ahora. Tu organización personal y los
                    datos locales siguen accesibles.
                  </p>
                )}
              </div>
              <div className="detail-overview__tags">
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
            <Tabs defaultValue="plan" className="detail-tabs">
              <TabsList>
                <TabsTrigger value="plan">Plan personal</TabsTrigger>
                <TabsTrigger value="info">Información</TabsTrigger>
                <TabsTrigger value="journal">Registro</TabsTrigger>
                <TabsTrigger value="history">Actividad</TabsTrigger>
              </TabsList>
              <TabsContent value="plan">
                <DetailForm
                  detail={detail}
                  statuses={statuses}
                  collections={collections}
                  watched={watched}
                  form={form}
                  mutation={mutation}
                  collectionMutation={collectionMutation}
                />
              </TabsContent>
              <TabsContent value="info" className="detail-info">
                <InfoRow label="Desarrollador" value={detail.developer} />
                <InfoRow label="Editor" value={detail.publisher} />
                <InfoRow label="Lanzamiento" value={displayDate(detail.releaseDate)} />
                <InfoRow label="Modelo" value={detail.isFree ? "Free to Play" : "De pago"} />
                <InfoRow label="Propiedad" value={ownershipLabel(detail.ownershipSource)} />
                {detail.ownershipSource === "family_shared" && (
                  <InfoRow
                    label="Disponibilidad familiar"
                    value={familyAvailabilityLabel(detail.familyAvailability)}
                  />
                )}
                <InfoRow label="Instalación" value={detail.installPath} />
                <InfoRow label="Categorías" value={detail.categories.join(", ")} />
                <InfoRow label="Géneros" value={detail.genres.join(", ")} />
                <InfoRow label="Ficha actualizada" value={displayDate(detail.metadataFetchedAt)} />
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
  mutation,
  collectionMutation,
}: {
  detail: GameDetail;
  statuses: StatusDefinition[];
  collections: CollectionSummary[];
  watched: Partial<DetailValues>;
  form: ReturnType<typeof useForm<DetailValues>>;
  mutation: ReturnType<typeof useMutation<GameDetail, Error, UpdateGameInput>>;
  collectionMutation: ReturnType<typeof useMutation<GameDetail, Error, string[]>>;
}) {
  return (
    <form className="detail-form" onSubmit={form.handleSubmit((values) => mutation.mutate(values))}>
      <div className="detail-form__row">
        <div className="detail-field">
          <span>Estado</span>
          <Select
            value={watched.statusId ?? "unclassified"}
            onValueChange={(value) => form.setValue("statusId", value, { shouldDirty: true })}
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
            onValueChange={(value) =>
              form.setValue("rating", value === "none" ? undefined : Number(value), {
                shouldDirty: true,
              })
            }
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
          <Input
            id="game-estimated-minutes"
            type="number"
            min={1}
            step={15}
            placeholder="Minutos"
            {...form.register("estimatedMinutes", {
              setValueAs: (value) => (value === "" ? undefined : Number(value)),
            })}
          />
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
      </label>
      <label htmlFor="game-next-action">
        <span>
          <IconTargetArrow size={14} /> Próxima acción
        </span>
        <Input
          id="game-next-action"
          placeholder="La siguiente cosa concreta que quieres hacer"
          {...form.register("nextAction")}
        />
      </label>
      <label htmlFor="game-notes">
        <span>Notas privadas</span>
        <Textarea
          id="game-notes"
          rows={5}
          placeholder="Decisiones, estrategia o contexto que quieras conservar"
          {...form.register("notes")}
        />
      </label>
      <div className="detail-toggles">
        <div className="detail-toggle">
          <Switch
            aria-label="Fijar en la biblioteca"
            checked={watched.pinned ?? false}
            onCheckedChange={(pinned) => form.setValue("pinned", pinned, { shouldDirty: true })}
          />
          <span>Fijado en la biblioteca</span>
        </div>
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
      <Button type="submit" size="sm" disabled={!form.formState.isDirty || mutation.isPending}>
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
function DetailMetric({
  label,
  value,
  icon,
  action,
}: {
  label: string;
  value: string;
  icon?: React.ReactNode;
  action?: React.ReactNode;
}) {
  return (
    <div>
      {icon}
      <span>{label}</span>
      <strong>{value}</strong>
      {action}
    </div>
  );
}
function InfoRow({ label, value }: { label: string; value: string | undefined }) {
  return (
    <div className="info-row">
      <span>{label}</span>
      <strong>{value || "—"}</strong>
    </div>
  );
}

function displayDate(value?: string): string {
  return value ? formatDate(value) : "Sin datos";
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
