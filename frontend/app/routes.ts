import { type RouteConfig, index, layout, route } from "@react-router/dev/routes";

export default [
  route("auth", "routes/auth.tsx"),
  layout("routes/layout.tsx", [
    index("routes/home.tsx"),
    route("chat/:chatId", "routes/chat.tsx"),
    route("scheduled", "routes/scheduled.tsx"),
    route("library", "routes/library.tsx"),
    route("account", "routes/account.tsx"),
    route("admin", "routes/admin/layout.tsx", [
      index("routes/admin/overview.tsx"),
      route("users", "routes/admin/users.tsx"),
      route("workspaces", "routes/admin/workspaces.tsx"),
      route("library", "routes/admin/library.tsx"),
      route("usage", "routes/admin/usage.tsx"),
      route("logs", "routes/admin/logs.tsx"),
    ]),
  ]),
] satisfies RouteConfig;
