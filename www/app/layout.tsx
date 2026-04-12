import type { Metadata } from "next";
import { GeistMono } from "geist/font/mono";
import { GeistSans } from "geist/font/sans";
import "./globals.css";

export const metadata: Metadata = {
  title: "ctx",
  description: "ctx",
};

export default function RootLayout({
  children,
}: Readonly<{
  children: React.ReactNode;
}>) {
  return (
    <html lang="en" className="h-[100vh]">
      <body
        className={`${GeistSans.variable} ${GeistMono.variable} h-[100vh] min-h-[100vh] antialiased`}
      >
        {children}
      </body>
    </html>
  );
}
