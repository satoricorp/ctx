import { berkeleyMono } from "./fonts";
import { InstallCommand } from "./install-command";
import { Logo } from "./logo";
import { ThemeToggle } from "./theme-toggle";

export default function Home() {
  return (
    <div className="relative isolate min-h-[100vh]">
      <div className="relative z-10 flex min-h-[100vh] flex-col items-center justify-center gap-6 px-4">
        <div className="absolute inset-x-[25%] top-1/4 flex flex-col items-start gap-6">
          <Logo />
          <p
            className={`${berkeleyMono.className} text-muted-foreground max-w-prose text-xs uppercase leading-relaxed tracking-[0.2em]`}
          >
            <span>§</span>
            <span className="ml-2">Spec / 0.1 — Portable Context Protocol</span>
          </p>
          <div className="flex w-full justify-center">
            <InstallCommand />
          </div>
        </div>
        <footer className="absolute inset-x-0 bottom-0 flex flex-col pb-2 pt-2">
          <div className="bg-foreground/10 mb-2 h-px w-full shrink-0" aria-hidden />
          <div className="flex min-h-9 items-center justify-between gap-4 px-8">
            <a
              href="https://satori.sh"
              className={`${berkeleyMono.className} flex items-center gap-1.5 text-foreground text-xs leading-none tracking-wide transition-colors hover:text-[var(--accent-highlight)]`}
            >
              <span>©</span>
              <span style={{ color: "var(--accent-highlight)" }} aria-hidden>
                ⏺
              </span>
              <span>Satori Engineering Co.</span>
            </a>
            <ThemeToggle />
          </div>
        </footer>
      </div>
    </div>
  );
}
