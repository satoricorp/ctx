import type { Metadata } from "next";
import Script from "next/script";
import { GeistMono } from "geist/font/mono";
import { Nav } from "./nav";
import { ThemeInit } from "./theme-init";
import "./globals.css";
import { berkeleyMono } from "./fonts";
import { cn } from "@/lib/utils";

const GA_MEASUREMENT_ID = "G-BQK5SZ9YL4";

export const metadata: Metadata = {
  title: "CTX: Local Context for Agents and Humans",
  description:
    "CTX is local context for agents and humans with plain markdown notes, local indexing, MCP, and installable skills.",
};

export default function RootLayout({
  children,
}: Readonly<{
  children: React.ReactNode;
}>) {
  return (
    <html
      lang="en"
      suppressHydrationWarning
      className={cn("h-[100vh]", "font-sans", berkeleyMono.variable)}
    >
      <body
        className={`${berkeleyMono.className} ${berkeleyMono.variable} ${GeistMono.variable} h-[100vh] min-h-[100vh] antialiased`}
      >
        <Script
          src={`https://www.googletagmanager.com/gtag/js?id=${GA_MEASUREMENT_ID}`}
          strategy="afterInteractive"
        />
        <Script id="google-analytics" strategy="afterInteractive">
          {`
            window.dataLayer = window.dataLayer || [];
            function gtag(){dataLayer.push(arguments);}
            gtag('js', new Date());
            gtag('config', '${GA_MEASUREMENT_ID}');
          `}
        </Script>
        <ThemeInit />
        <Nav />
        {children}
      </body>
    </html>
  );
}
