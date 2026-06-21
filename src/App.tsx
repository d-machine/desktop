import { useEffect, useState } from "react";
import "./App.css";
import { SetupScreen } from "@/components/auth/SetupScreen";
import { LoginScreen } from "@/components/auth/LoginScreen";
import { AppLayout } from "@/components/layout/AppLayout";
import { OnboardingWizard } from "@/components/onboarding/OnboardingWizard";
import { apiGet, initBackendListener, setSessionToken, setPort } from "@/lib/api";
import { invoke } from "@tauri-apps/api/core";

type AppState = "waiting" | "loading" | "setup" | "locked" | "onboarding" | "unlocked";

function App() {
  const [appState, setAppState]     = useState<AppState>("waiting");
  const [crashMsg, setCrashMsg]     = useState<string | null>(null);

  // Wait for Tauri to emit "backend-ready" before doing anything
  useEffect(() => {
    // Register event listener first (for future events)
    initBackendListener(
      (port) => {
        setPort(port);
        setAppState("loading");
        checkAuthState();
      },
      (reason) => setCrashMsg(reason),
    );

    // Then immediately query Rust — handles the race where the event
    // fired before this listener was registered
    invoke<number | null>("get_backend_port").then((port) => {
      if (port != null) {
        setPort(port);
        setAppState("loading");
        checkAuthState();
      }
    }).catch(() => {});
  }, []);

  async function checkAuthState() {
    try {
      const status = await apiGet<{ setup: boolean; locked: boolean }>("/auth/status");
      if (!status.setup) {
        setAppState("setup");
        return;
      }
      if (status.locked) {
        setAppState("locked");
        return;
      }
      await checkPortfolios();
    } catch {
      setAppState("locked");
    }
  }

  async function checkPortfolios() {
    const portfolios = await apiGet<{ portfolio_id: number }[]>("/portfolios");
    setAppState(portfolios.length === 0 ? "onboarding" : "unlocked");
  }

  function handleSetupComplete() {
    setAppState("onboarding");
  }

  function handleLoginComplete(token: string) {
    setSessionToken(token);
    checkPortfolios().then(() => {});
  }

  function handleLock() {
    setSessionToken("");
    setAppState("locked");
  }

  // Fatal crash modal
  if (crashMsg) {
    return (
      <div className="min-h-screen bg-background flex items-center justify-center">
        <div className="text-center space-y-4 p-8 max-w-md">
          <p className="text-lg font-semibold text-destructive">Backend process crashed</p>
          <p className="text-muted-foreground text-sm">{crashMsg}</p>
          <p className="text-muted-foreground text-sm">Please restart the application.</p>
        </div>
      </div>
    );
  }

  if (appState === "waiting" || appState === "loading") {
    return (
      <div className="min-h-screen bg-background flex items-center justify-center">
        <p className="text-muted-foreground">
          {appState === "waiting" ? "Starting backend…" : "Loading…"}
        </p>
      </div>
    );
  }

  if (appState === "setup") {
    return <SetupScreen onComplete={() => handleSetupComplete()} />;
  }

  if (appState === "locked") {
    return <LoginScreen onUnlocked={handleLoginComplete} />;
  }

  if (appState === "onboarding") {
    return <OnboardingWizard onComplete={() => setAppState("unlocked")} />;
  }

  return <AppLayout onLock={handleLock} />;
}

export default App;
