import localFont from "next/font/local";

export const blobLogo = localFont({
  src: "../public/fonts/Blob.woff2",
  display: "swap",
});

export const berkeleyMono = localFont({
  src: "../public/fonts/BerkeleyMonoVariable.otf",
  display: "swap",
  variable: "--font-berkeley-mono",
});
