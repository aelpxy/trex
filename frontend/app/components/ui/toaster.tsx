import type { ReactNode } from "react";
import { Toast } from "@base-ui/react/toast";
import { LuCircleAlert, LuCircleCheck, LuLoaderCircle, LuX } from "react-icons/lu";

import { toasts } from "~/lib/toasts";

import { focusRing } from "./styles";

const ICONS = {
  success: <LuCircleCheck size={16} className="shrink-0 text-ink" />,
  error: <LuCircleAlert size={16} className="shrink-0 text-danger" />,
  loading: <LuLoaderCircle size={16} className="shrink-0 animate-spin text-muted" />,
};

function ToastList() {
  const { toasts: list } = Toast.useToastManager();
  return list.map((toast) => (
    <Toast.Root
      key={toast.id}
      toast={toast}
      swipeDirection={["right", "down"]}
      className="glass flex w-full items-start gap-3 rounded-xl border border-line p-3.5 shadow-lg transition-[transform,opacity] duration-200 data-ending-style:translate-y-2 data-ending-style:opacity-0 data-starting-style:translate-y-2 data-starting-style:opacity-0"
    >
      {ICONS[toast.type as keyof typeof ICONS]}
      <Toast.Content className="min-w-0 flex-1">
        <Toast.Title className="text-[13px] font-medium" />
        <Toast.Description className="mt-0.5 text-xs break-words text-muted" />
      </Toast.Content>
      <Toast.Close aria-label="Dismiss" className={`-m-1 shrink-0 cursor-pointer rounded-md p-1 text-muted hover:text-ink ${focusRing}`}>
        <LuX size={14} />
      </Toast.Close>
    </Toast.Root>
  ));
}

// where every toast shows: bottom right, above the page, newest last
export function Toaster({ children }: { children: ReactNode }) {
  return (
    <Toast.Provider toastManager={toasts} limit={4}>
      {children}
      <Toast.Portal>
        <Toast.Viewport className="fixed right-4 bottom-4 z-[60] flex w-[calc(100vw-2rem)] max-w-sm flex-col gap-2 outline-none">
          <ToastList />
        </Toast.Viewport>
      </Toast.Portal>
    </Toast.Provider>
  );
}
