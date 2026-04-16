import { GeistMono } from "geist/font/mono";
import { blobLogo } from "./fonts";
import { InstallCommand } from "./install-command";
import { Logo } from "./logo";

export default function Home() {
  return (
    <div className="relative flex min-h-[100vh] flex-col items-center justify-center gap-6 px-4">
      <div className="absolute left-1/4 top-1/4 flex flex-col items-start gap-2">
        <Logo />
        <p
          className={`${GeistMono.className} text-xl text-slate-800 dark:text-slate-200`}
        >
          Docker
          <br />
          for context.
        </p>
      </div>
      <InstallCommand />
      <footer className="absolute bottom-4 left-8">
        <a
          href="https://satori.sh"
          className={`${blobLogo.className} text-foreground text-sm tracking-wide transition-colors hover:text-[#ec4899]`}
        >
          © Satori Engineering Co
        </a>
      </footer>
    </div>
  );
}
