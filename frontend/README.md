
The home route is a centered chat preview; `/design-system` showcases the design system. Shared components live in
`app/components/ui/index.tsx`; color, typography, radius, and component styles
live in `app/app.css`. Use semantic Tailwind tokens such as `bg-canvas`,
`text-ink`, `text-muted`, and `border-line`. The interface uses self-hosted Geist
and Geist Mono with preloaded variable fonts. Demo workspace preferences are
kept in memory for the current session.

The neutral palette supports light and dark themes. The header toggle remembers
your choice locally; without a saved choice, it follows your system theme.

Chat components live in `app/components/chat`. Conversation state uses a pure
reducer with typed messages. The chat currently provides preview responses;
an AI service is not connected. Messages remain in memory until the page reloads.

The chat sidebar supports a desktop icon rail and mobile drawer, folders, chat
switching, and a Library of completed responses. Chats and folders are kept in
session memory. Workspace and account labels are placeholders until account
data is connected.
