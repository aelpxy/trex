import { useMutation, useQueryClient, useSuspenseQuery } from "@tanstack/react-query";

import { Button } from "~/components/ui/button";
import { queries } from "~/lib/queries";
import { toastOutcome } from "~/lib/toasts";
import { trex, type ApiSignInSession } from "~/lib/trex";

import { SessionList } from "./session-list";

export function Devices() {
  const queryClient = useQueryClient();
  const { data: sessions } = useSuspenseQuery(queries.signInSessions());
  const refresh = () => queryClient.invalidateQueries({ queryKey: queries.signInSessions().queryKey });
  const end = useMutation({
    mutationFn: (session: ApiSignInSession) => trex.endSignInSession(session.id),
    // signing out this device ends the session the app runs on
    onSuccess: (_, session) => (session.current ? window.location.assign("/auth") : refresh()),
  });
  const others = useMutation({ mutationFn: trex.signOutOthers, onSuccess: refresh });
  const hasOthers = sessions.some((session) => !session.current);

  return (
    <div className="space-y-3">
      <SessionList
        sessions={sessions}
        onEnd={(session) => void toastOutcome(end.mutateAsync(session), { success: "Device signed out", error: "Couldn't sign that device out" })}
        ending={end.isPending ? end.variables.id : null}
      />
      <Button variant="quiet" onClick={() => void toastOutcome(others.mutateAsync(), { success: "Other devices signed out", error: "Couldn't sign the other devices out" })} disabled={!hasOthers || others.isPending}>
        Sign out all other devices
      </Button>
    </div>
  );
}
