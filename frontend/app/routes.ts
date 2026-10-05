import { type RouteConfig, index, layout, route } from "@react-router/dev/routes";

export default [
  route("auth", "routes/auth.tsx"),
  layout("routes/layout.tsx", [
    index("routes/home.tsx"),
    route("chat/:chatId", "routes/chat.tsx"),
    route("scheduled", "routes/scheduled.tsx"),
    route("library", "routes/library.tsx"),
    route("account", "routes/account.tsx"),
    route("admin", "routes/admin.tsx"),
  ]),
] satisfies RouteConfig;
