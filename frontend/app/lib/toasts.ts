import { Toast } from "@base-ui/react/toast";

// one manager for the whole app, so mutations and handlers can report without a hook
export const toasts = Toast.createToastManager();

export const errorMessage = (cause: unknown) => (cause instanceof Error ? cause.message : String(cause));

// a toast that follows a piece of work: a spinner line while it runs, then the outcome
export function trackToast<T>(work: Promise<T>, messages: { loading: string; success: string; error: string }) {
  return toasts.promise(work, {
    loading: { title: messages.loading, type: "loading" },
    success: { title: messages.success, type: "success" },
    error: (cause) => ({ title: messages.error, description: errorMessage(cause), type: "error" }),
  });
}
