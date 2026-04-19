"use client";

import { GeistMono } from "geist/font/mono";
import { Check, Copy } from "lucide-react";
import { useCallback, useState } from "react";

const INSTALL_CMD = "cargo install ctx";

export function InstallCommand() {
  const [copied, setCopied] = useState(false);

  const copy = useCallback(async () => {
    try {
      await navigator.clipboard.writeText(INSTALL_CMD);
      setCopied(true);
      window.setTimeout(() => setCopied(false), 2000);
    } catch {
      setCopied(false);
    }
  }, []);

  return (
    <div
      className={`${GeistMono.className} border-border bg-muted text-foreground flex w-full max-w-xl items-center gap-3 rounded-xl border px-4 py-3`}
    >
      <code className="min-w-0 flex-1 truncate text-sm">{INSTALL_CMD}</code>
      <button
        type="button"
        onClick={copy}
        aria-label={copied ? "Copied" : "Copy install command"}
        className="border-border bg-background text-foreground hover:bg-muted shrink-0 rounded-lg border p-2 transition"
      >
        {copied ? (
          <Check className="size-4" aria-hidden />
        ) : (
          <Copy className="size-4" aria-hidden />
        )}
      </button>
    </div>
  );
}
