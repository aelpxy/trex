import { Page } from "~/components/ui/page";
import { pageTitle } from "~/lib/meta";

export const meta = () => pageTitle("Admin");

export default function Admin() {
  return <Page title="Admin" />;
}
