export function LoadingState({ label = "Cargando biblioteca" }: { label?: string }) {
  return (
    <div className="loading-state" role="status" aria-live="polite">
      <span className="loading-state__mark" aria-hidden="true" />
      <span>{label}</span>
    </div>
  );
}
