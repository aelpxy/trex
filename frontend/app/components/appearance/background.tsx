import { useAppearance } from "./appearance-provider";

export function Background() {
  const { backgroundUrl } = useAppearance();
  if (!backgroundUrl) return null;
  return <div aria-hidden className="fixed inset-0 -z-10 bg-cover bg-center" style={{ backgroundImage: `url("${backgroundUrl}")` }} />;
}
