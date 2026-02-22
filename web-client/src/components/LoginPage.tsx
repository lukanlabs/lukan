import { Eye, EyeOff, AlertCircle } from "lucide-react";
import React, { useState, useCallback } from "react";
import logoUrl from "../assets/logo.png";
import { Button } from "@/components/ui/button";

interface LoginPageProps {
  onLogin: (password: string) => void;
  error: string | null;
}

export function LoginPage({ onLogin, error }: LoginPageProps) {
  const [password, setPassword] = useState("");
  const [showPassword, setShowPassword] = useState(false);

  const handleSubmit = useCallback(
    (e: React.FormEvent) => {
      e.preventDefault();
      if (password.trim()) {
        onLogin(password);
      }
    },
    [password, onLogin],
  );

  return (
    <div className="flex h-screen flex-col items-center justify-center bg-zinc-950 px-4">
      <div className="w-full max-w-sm">
        {/* Logo / Icon */}
        <div className="flex flex-col items-center gap-4 mb-8">
          <div className="flex h-16 w-16 items-center justify-center rounded-2xl bg-zinc-800 border border-zinc-700">
            <img src={logoUrl} alt="lukan" className="h-9 w-9" />
          </div>
          <div className="text-center">
            <h1 className="text-xl font-semibold text-zinc-100">lukan</h1>
            <p className="text-sm text-zinc-500 mt-1">Enter your password to continue</p>
          </div>
        </div>

        {/* Login form */}
        <form onSubmit={handleSubmit} className="space-y-4">
          <div className="relative">
            <input
              type={showPassword ? "text" : "password"}
              value={password}
              onChange={(e) => setPassword(e.target.value)}
              placeholder="Password"
              autoFocus
              className="w-full rounded-lg border border-zinc-700 bg-zinc-800/50 px-4 py-3 pr-10 text-sm text-zinc-100 placeholder-zinc-500 outline-none focus:border-purple-500/50 focus:ring-1 focus:ring-purple-500/25 transition-colors"
            />
            <button
              type="button"
              onClick={() => setShowPassword(!showPassword)}
              className="absolute right-3 top-1/2 -translate-y-1/2 text-zinc-500 hover:text-zinc-300 transition-colors"
            >
              {showPassword ? <EyeOff className="h-4 w-4" /> : <Eye className="h-4 w-4" />}
            </button>
          </div>

          {error && (
            <div className="flex items-center gap-2 rounded-lg border border-red-500/20 bg-red-500/10 px-3 py-2">
              <AlertCircle className="h-4 w-4 text-red-400 shrink-0" />
              <span className="text-xs text-red-300">{error}</span>
            </div>
          )}

          <Button
            type="submit"
            className="w-full bg-purple-600 hover:bg-purple-500 text-white py-2.5"
            disabled={!password.trim()}
          >
            Sign in
          </Button>
        </form>
      </div>
    </div>
  );
}
