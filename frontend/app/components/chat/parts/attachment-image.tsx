import { useEffect, useState } from "react";

import { focusRing } from "~/components/ui/styles";
import { attachment } from "~/lib/api";

// a tool's text marks each image it returned as `[image: attachment://<sha256>#<mime>]`
const IMAGE = /\[image: attachment:\/\/([0-9a-f]{64})#[^\]]*\]/g;

export const imagesIn = (output: string) => [...output.matchAll(IMAGE)].map((match) => match[1]);
export const withoutImages = (output: string) => output.replace(IMAGE, "").replace(/\n{3,}/g, "\n\n").trim();

// an image the agent saw, loaded with the session's cookie; opens full size in a new tab
export function AttachmentImage({ hash, alt }: { hash: string; alt: string }) {
  const [url, setUrl] = useState<string>();
  useEffect(() => {
    let current: string | undefined;
    let cancelled = false;
    attachment(`att_${hash}`)
      .then((blob) => {
        if (cancelled) return;
        current = URL.createObjectURL(blob);
        setUrl(current);
      })
      .catch((error) => console.warn("could not load the image", error));
    return () => {
      cancelled = true;
      if (current) URL.revokeObjectURL(current);
    };
  }, [hash]);
  if (!url) return <div className="aspect-video w-full max-w-md animate-pulse rounded-lg bg-subtle" aria-label="Loading screenshot" />;
  return (
    <a href={url} target="_blank" rel="noreferrer" title="Open full size" className={`block w-fit rounded-lg ${focusRing}`}>
      <img src={url} alt={alt} className="max-h-80 rounded-lg border border-line" />
    </a>
  );
}
