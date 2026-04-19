import type { Metadata } from "next";
import Script from "next/script";
import { GeistMono } from "geist/font/mono";
import { GeistSans } from "geist/font/sans";
import { Nav } from "./nav";
import { ThemeInit } from "./theme-init";
import "./globals.css";
import { Geist } from "next/font/google";
import { cn } from "@/lib/utils";

const geist = Geist({subsets:['latin'],variable:'--font-sans'});

const GA_MEASUREMENT_ID = "G-BQK5SZ9YL4";

export const metadata: Metadata = {
  title: "CTX: Portable Context Protocol",
  description: "ctx",
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
      className={cn("h-[100vh]", "font-sans", geist.variable)}
    >
      <body
        className={`${GeistSans.variable} ${GeistMono.variable} h-[100vh] min-h-[100vh] antialiased`}
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
