import { Devices } from "~/components/account/devices";
import { Section } from "~/components/account/section";
import { queries } from "~/lib/queries";
import { queryClient } from "~/lib/query-client";

export async function clientLoader() {
  await queryClient.ensureQueryData(queries.signInSessions());
  return null;
}

export default function AccountDevices() {
  return (
    <Section title="Devices" description="Browsers signed in to your account. Sign out any you don't recognize.">
      <Devices />
    </Section>
  );
}
