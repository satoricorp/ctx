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
      className={`${GeistMono.className} flex w-full max-w-xl items-center gap-3 rounded-xl bg-slate-100 px-4 py-3 text-slate-800 dark:bg-zinc-800 dark:text-zinc-100`}
    >
      <code className="min-w-0 flex-1 truncate text-sm">{INSTALL_CMD}</code>
      <button
        type="button"
        onClick={copy}
        aria-label={copied ? "Copied" : "Copy install command"}
        className="shrink-0 rounded-lg border border-slate-200 bg-white p-2 text-slate-700 transition hover:bg-slate-50 dark:border-zinc-600 dark:bg-zinc-700 dark:text-zinc-100 dark:hover:bg-zinc-600"
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
