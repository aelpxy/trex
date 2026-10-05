import { FileManager } from "~/components/library/file-manager";
import { Page } from "~/components/ui/page";
import { pageTitle } from "~/lib/meta";
import { queries } from "~/lib/queries";
import { queryClient } from "~/lib/query-client";

export const meta = () => pageTitle("Library");

export async function clientLoader() {
  await queryClient.ensureQueryData(queries.library());
  return null;
}

export default function Library() {
  return (
    <Page title="Library">
      <FileManager />
    </Page>
  );
}
