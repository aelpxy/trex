import {
  isRouteErrorResponse,
  Links,
  Meta,
  Outlet,
  Scripts,
  ScrollRestoration,
} from "react-router";

import type { Route } from "./+types/root";
import geistFont from "@fontsource-variable/geist/files/geist-latin-wght-normal.woff2?url";
import geistMonoFont from "@fontsource-variable/geist-mono/files/geist-mono-latin-wght-normal.woff2?url";
import { APP_NAME, pageTitle } from "~/lib/meta";

import "./app.css";

export const meta: Route.MetaFunction = () => pageTitle();

export const links: Route.LinksFunction = () => [
  { rel: "preload", href: geistFont, as: "font", type: "font/woff2", crossOrigin: "anonymous" },
  { rel: "preload", href: geistMonoFont, as: "font", type: "font/woff2", crossOrigin: "anonymous" },
  { rel: "icon", href: "/favicon.ico", sizes: "32x32" },
  { rel: "icon", href: "/icon-192.png", type: "image/png" },
  { rel: "apple-touch-icon", href: "/apple-touch-icon.png" },
  { rel: "manifest", href: "/manifest.webmanifest" },
];

export function Layout({ children }: { children: React.ReactNode }) {
  return (
    <html lang="en" suppressHydrationWarning>
      <head>
        <meta charSet="utf-8" />
        <meta name="viewport" content="width=device-width, initial-scale=1" />
        <script dangerouslySetInnerHTML={{ __html: `(() => { let theme; try { theme = localStorage.getItem("trex-theme"); } catch {} document.documentElement.dataset.theme = theme === "light" || theme === "dark" ? theme : matchMedia("(prefers-color-scheme: dark)").matches ? "dark" : "light"; })();` }} />
        <Meta />
        <Links />
      </head>
      <body>
        <div className="isolate min-h-svh">{children}</div>
        <ScrollRestoration />
        <Scripts />
      </body>
    </html>
  );
}

export default function App() {
  return <Outlet />;
}

export function HydrateFallback() {
  return <div className="min-h-svh" />;
}

export function ErrorBoundary({ error }: Route.ErrorBoundaryProps) {
  const notFound = isRouteErrorResponse(error) && error.status === 404;
  const status = isRouteErrorResponse(error) ? String(error.status) : "Error";
  const details = notFound
    ? "This page doesn't exist or was moved."
    : isRouteErrorResponse(error)
      ? error.statusText || "Something went wrong."
      : import.meta.env.DEV && error instanceof Error
        ? error.message
        : "Something went wrong. Try again in a moment.";
  const stack = import.meta.env.DEV && error instanceof Error ? error.stack : undefined;

  return (
    <main className="flex min-h-svh flex-col items-center justify-center px-6 text-center">
      <title>{`${notFound ? "Page not found" : "Error"} · ${APP_NAME}`}</title>
      <p className="eyebrow">{status}</p>
      <h1 className="mt-3 text-2xl font-medium tracking-tight">{notFound ? "Page not found" : "Something went wrong"}</h1>
      <p className="mt-2 max-w-sm text-sm text-muted">{details}</p>
      <a href="/" className="ui-button mt-6 h-9 bg-ink px-4 text-on-solid hover:bg-ink/85 focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-accent">
        Back to {APP_NAME}
      </a>
      {stack && <pre className="mt-8 max-w-3xl overflow-x-auto rounded-xl border border-line bg-surface p-4 text-left font-mono text-xs leading-6">{stack}</pre>}
    </main>
  );
}
