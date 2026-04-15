import { GeistMono } from "geist/font/mono";
import localFont from "next/font/local";
import { InstallCommand } from "./install-command";

const blobLogo = localFont({
  src: "../public/fonts/Blob.woff2",
  display: "swap",
});

export default function Home() {
  return (
    <div className="flex min-h-[100vh] flex-col items-center justify-center gap-6 px-4">
      <div className="flex flex-col items-center gap-2">
        <p
          className={`${blobLogo.className} text-foreground text-[180px] leading-none font-medium`}
        >
          ctx
        </p>
        <p
          className={`${GeistMono.className} text-center text-xl text-slate-800 dark:text-slate-200`}
        >
          Docker for context.
        </p>
      </div>
      <InstallCommand />
    </div>
  );
}
