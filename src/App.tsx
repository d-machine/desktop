import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import "./App.css";
import { SetupScreen } from "@/components/auth/SetupScreen";
import { LoginScreen } from "@/components/auth/LoginScreen";
import { AppLayout } from "@/components/layout/AppLayout";
import { OnboardingWizard } from "@/components/onboarding/OnboardingWizard";

type AppState = "loading" | "setup" | "locked" | "onboarding" | "unlocked";

function App() {
  const [appState, setAppState] = useState<AppState>("loading");

  useEffect(() => {
    async function checkAuthState() {
      const setup = await invoke<boolean>("is_setup");
      if (!setup) {
        setAppState("setup");
        return;
      }
      const unlocked = await invoke<boolean>("is_unlocked");
      if (!unlocked) { setAppState("locked"); return; }
      // Check if user has any portfolios yet
      const portfolios = await invoke<{ portfolio_id: number }[]>("get_portfolios");
      setAppState(portfolios.length === 0 ? "onboarding" : "unlocked");
    }
    checkAuthState();
  }, []);

  if (appState === "loading") {
    return (
      <div className="min-h-screen bg-background flex items-center justify-center">
        <p className="text-muted-foreground">Loading…</p>
      </div>
    );
  }

  if (appState === "setup") {
    return <SetupScreen onComplete={() => setAppState("unlocked")} />;
  }

  if (appState === "locked") {
    return <LoginScreen onUnlocked={async () => {
      const portfolios = await invoke<{ portfolio_id: number }[]>("get_portfolios");
      setAppState(portfolios.length === 0 ? "onboarding" : "unlocked");
    }} />;
  }

  if (appState === "onboarding") {
    return <OnboardingWizard onComplete={() => setAppState("unlocked")} />;
  }

  // Unlocked — main app
  return <AppLayout onLock={() => setAppState("locked")} />;
}

export default App;
