import { BalanceCard, Ledger } from "~/components/account/balance";
import { Section } from "~/components/account/section";
import { queries } from "~/lib/queries";
import { queryClient } from "~/lib/query-client";

import type { Route } from "./+types/billing";

// the page lives in the url, so the loader has the entries ready before the table renders
export async function clientLoader({ request }: Route.ClientLoaderArgs) {
  const page = Math.max(1, Number(new URL(request.url).searchParams.get("page")) || 1);
  await Promise.all([queryClient.ensureQueryData(queries.credits()), queryClient.ensureQueryData(queries.ledger(page))]);
  return null;
}

export default function AccountBilling() {
  return (
    <>
      <Section title="Balance" description="Each model response is charged by its tokens.">
        <BalanceCard />
      </Section>
      <Section title="Activity">
        <Ledger />
      </Section>
    </>
  );
}
