import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { useState } from "react";
import { ErrorBoundary } from "@/components/common/ErrorBoundary";
import { TooltipProvider } from "@/components/ui/tooltip";
import { DatabaseRecoveryGate } from "@/features/recovery/DatabaseRecoveryGate";
import { AppShell } from "@/features/shell/AppShell";

const queryClient = new QueryClient({
  defaultOptions: {
    queries: { retry: 1, staleTime: 20_000, refetchOnWindowFocus: false },
    mutations: { retry: 0 },
  },
});

export function VindexaApp() {
  const [errorResetKey, setErrorResetKey] = useState(0);
  return (
    <ErrorBoundary key={errorResetKey} onReset={() => setErrorResetKey((key) => key + 1)}>
      <QueryClientProvider client={queryClient}>
        <TooltipProvider delayDuration={280} skipDelayDuration={100}>
          <DatabaseRecoveryGate>
            <AppShell />
          </DatabaseRecoveryGate>
        </TooltipProvider>
      </QueryClientProvider>
    </ErrorBoundary>
  );
}
