import { type RouteConfig, index, layout, route } from "@react-router/dev/routes";

export default [
  route("auth", "routes/auth.tsx"),
  layout("routes/layout.tsx", [
    index("routes/home.tsx"),
    route("chat/:chatId", "routes/chat.tsx"),
    route("projects/:projectId", "routes/project.tsx"),
    route("scheduled", "routes/scheduled.tsx"),
    route("scheduled/:taskId", "routes/scheduled-task.tsx"),
    route("library", "routes/library.tsx"),
    route("account", "routes/account/layout.tsx", [
      index("routes/account/billing.tsx"),
      route("profile", "routes/account/profile.tsx"),
      route("devices", "routes/account/devices.tsx"),
      route("appearance", "routes/account/appearance.tsx"),
    ]),
    route("admin", "routes/admin/layout.tsx", [
      index("routes/admin/overview.tsx"),
      route("users", "routes/admin/users.tsx"),
      route("workspaces", "routes/admin/workspaces.tsx"),
      route("library", "routes/admin/library.tsx"),
      route("runs", "routes/admin/runs.tsx"),
      route("sandboxes", "routes/admin/sandboxes.tsx"),
      route("usage", "routes/admin/usage.tsx"),
      route("logs", "routes/admin/logs.tsx"),
    ]),
  ]),
] satisfies RouteConfig;
