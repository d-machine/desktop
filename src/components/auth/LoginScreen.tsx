import { useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { PinInput } from "./PinInput";
import { ForgotPinFlow } from "./ForgotPinFlow";

interface LoginScreenProps {
  onUnlocked: () => void;
}

export function LoginScreen({ onUnlocked }: LoginScreenProps) {
  const [pin, setPin] = useState("");
  const [error, setError] = useState("");
  const [loading, setLoading] = useState(false);
  const [showForgot, setShowForgot] = useState(false);
  const [attempts, setAttempts] = useState(0);

  if (showForgot) {
    return <ForgotPinFlow onRecovered={onUnlocked} onBack={() => setShowForgot(false)} />;
  }

  const handleLogin = async () => {
    if (pin.length < 6) return;
    setLoading(true);
    setError("");
    try {
      await invoke("login", { pin });
      onUnlocked();
    } catch {
      const next = attempts + 1;
      setAttempts(next);
      setError(next >= 5
        ? "Too many wrong attempts. Use your recovery file to reset your PIN."
        : `Wrong PIN. ${5 - next} attempt${5 - next === 1 ? "" : "s"} remaining.`
      );
    } finally {
      setLoading(false);
      setPin("");
    }
  };

  return (
    <div className="min-h-screen bg-background flex items-center justify-center p-4">
      <Card className="w-full max-w-sm">
        <CardHeader className="text-center">
          <CardTitle className="text-2xl">Portfolio Tracker</CardTitle>
          <CardDescription>Enter your PIN to unlock</CardDescription>
        </CardHeader>
        <CardContent className="space-y-6">
          <PinInput
            onChange={(p) => { setPin(p); setError(""); }}
            error={!!error}
            disabled={loading || attempts >= 5}
          />
          {error && <p className="text-destructive text-sm text-center">{error}</p>}
          <Button
            className="w-full"
            onClick={handleLogin}
            disabled={pin.length < 6 || loading || attempts >= 5}
          >
            {loading ? "Unlocking…" : "Unlock"}
          </Button>
          <Button
            variant="ghost"
            className="w-full text-muted-foreground text-sm"
            onClick={() => setShowForgot(true)}
          >
            Forgot PIN?
          </Button>
        </CardContent>
      </Card>
    </div>
  );
}
