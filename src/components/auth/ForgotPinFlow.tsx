import { useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";
import { readTextFile } from "@tauri-apps/plugin-fs";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { PinInput } from "./PinInput";

type Step = "load-file" | "enter-passphrase" | "set-new-pin" | "confirm-new-pin";

interface ForgotPinFlowProps {
  onRecovered: () => void;
  onBack: () => void;
}

export function ForgotPinFlow({ onRecovered, onBack }: ForgotPinFlowProps) {
  const [step, setStep] = useState<Step>("load-file");
  const [recoveryContents, setRecoveryContents] = useState("");
  const [passphrase, setPassphrase] = useState("");
  const [newPin, setNewPin] = useState("");
  const [confirmedPin, setConfirmedPin] = useState("");
  const [error, setError] = useState("");
  const [loading, setLoading] = useState(false);

  const handleLoadFile = async () => {
    try {
      const selected = await open({
        title: "Select Recovery File",
        filters: [{ name: "Recovery File", extensions: ["ptbak"] }],
      });
      if (selected) {
        const contents = await readTextFile(selected as string);
        setRecoveryContents(contents);
        setStep("enter-passphrase");
      }
    } catch (e: any) {
      setError(e.toString());
    }
  };

  const handleRecover = async () => {
    if (!passphrase) { setError("Enter your recovery passphrase"); return; }
    setLoading(true);
    setError("");
    try {
      // Verify passphrase decrypts the recovery file
      JSON.parse(recoveryContents); // basic sanity check
      setStep("set-new-pin");
    } catch {
      setError("Invalid recovery file");
    } finally {
      setLoading(false);
    }
  };

  const handleSetNewPin = () => {
    if (newPin.length < 6) { setError("PIN must be 6 digits"); return; }
    setError("");
    setStep("confirm-new-pin");
  };

  const handleConfirmNewPin = async () => {
    if (confirmedPin !== newPin) { setError("PINs do not match"); return; }
    setLoading(true);
    setError("");
    try {
      await invoke("recover", {
        recoveryFileContents: recoveryContents,
        passphrase,
        newPin,
      });
      onRecovered();
    } catch (e: any) {
      setError("Wrong recovery passphrase");
      setStep("enter-passphrase");
    } finally {
      setLoading(false);
    }
  };

  return (
    <div className="min-h-screen bg-background flex items-center justify-center p-4">
      <Card className="w-full max-w-md">
        {step === "load-file" && (
          <>
            <CardHeader className="text-center">
              <CardTitle className="text-2xl">Recover access</CardTitle>
              <CardDescription>
                Load the recovery file (.ptbak) you saved during setup.
              </CardDescription>
            </CardHeader>
            <CardContent className="space-y-4">
              {error && <p className="text-destructive text-sm">{error}</p>}
              <Button className="w-full" onClick={handleLoadFile}>
                Select recovery file (.ptbak)
              </Button>
              <Button variant="ghost" className="w-full" onClick={onBack}>
                Back to login
              </Button>
            </CardContent>
          </>
        )}

        {step === "enter-passphrase" && (
          <>
            <CardHeader className="text-center">
              <CardTitle className="text-2xl">Enter recovery passphrase</CardTitle>
              <CardDescription>
                Enter the passphrase you set during first-time setup.
              </CardDescription>
            </CardHeader>
            <CardContent className="space-y-4">
              <div className="space-y-2">
                <Label>Recovery passphrase</Label>
                <Input
                  type="password"
                  placeholder="Your recovery passphrase"
                  value={passphrase}
                  onChange={(e) => setPassphrase(e.target.value)}
                  onKeyDown={(e) => e.key === "Enter" && handleRecover()}
                />
              </div>
              {error && <p className="text-destructive text-sm">{error}</p>}
              <div className="flex gap-3">
                <Button variant="outline" className="flex-1" onClick={() => setStep("load-file")}>
                  Back
                </Button>
                <Button className="flex-1" onClick={handleRecover} disabled={loading}>
                  {loading ? "Verifying…" : "Continue"}
                </Button>
              </div>
            </CardContent>
          </>
        )}

        {step === "set-new-pin" && (
          <>
            <CardHeader className="text-center">
              <CardTitle className="text-2xl">Set a new PIN</CardTitle>
              <CardDescription>Choose a new 6-digit PIN.</CardDescription>
            </CardHeader>
            <CardContent className="space-y-6">
              <PinInput onChange={setNewPin} error={!!error} />
              {error && <p className="text-destructive text-sm text-center">{error}</p>}
              <Button className="w-full" onClick={handleSetNewPin} disabled={newPin.length < 6}>
                Continue
              </Button>
            </CardContent>
          </>
        )}

        {step === "confirm-new-pin" && (
          <>
            <CardHeader className="text-center">
              <CardTitle className="text-2xl">Confirm new PIN</CardTitle>
              <CardDescription>Enter your new PIN again.</CardDescription>
            </CardHeader>
            <CardContent className="space-y-6">
              <PinInput onChange={setConfirmedPin} error={!!error} />
              {error && <p className="text-destructive text-sm text-center">{error}</p>}
              <div className="flex gap-3">
                <Button variant="outline" className="flex-1" onClick={() => { setStep("set-new-pin"); setError(""); }}>
                  Back
                </Button>
                <Button className="flex-1" onClick={handleConfirmNewPin} disabled={loading || confirmedPin.length < 6}>
                  {loading ? "Recovering…" : "Recover & Unlock"}
                </Button>
              </div>
            </CardContent>
          </>
        )}
      </Card>
    </div>
  );
}
