import { useMutation, useQueryClient, useSuspenseQuery } from "@tanstack/react-query";

import { Button } from "~/components/ui/button";
import { queries } from "~/lib/queries";
import { trex, type ApiSignInSession } from "~/lib/trex";

import { Status } from "./section";
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
      <SessionList sessions={sessions} onEnd={(session) => end.mutate(session)} ending={end.isPending ? end.variables.id : null} />
      <div className="flex items-center gap-3">
        <Button variant="quiet" onClick={() => others.mutate()} disabled={!hasOthers || others.isPending}>
          Sign out all other devices
        </Button>
        <Status error={end.error ?? others.error} saved={others.isSuccess ? "Other devices were signed out." : null} />
      </div>
    </div>
  );
}
