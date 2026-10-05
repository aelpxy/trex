import { useEffect, useState } from "react";
import { LuFileText } from "react-icons/lu";

import { attachmentUrl, type MessageAttachment } from "~/lib/attachments";

function AttachedImage({ attachment }: { attachment: MessageAttachment }) {
  const [url, setUrl] = useState(attachment.url);
  useEffect(() => {
    if (attachment.url || !attachment.id) return;
    let current = true;
    attachmentUrl(attachment.id)
      .then((next) => current && setUrl(next))
      .catch((error) => console.warn("could not load the attachment", error));
    return () => {
      current = false;
    };
  }, [attachment.url, attachment.id]);

  if (!url) return <div role="img" aria-label={attachment.name} className="size-24 animate-pulse rounded-xl bg-subtle" />;
  return (
    <a href={url} target="_blank" rel="noopener noreferrer">
      <img src={url} alt={attachment.name} className="max-h-48 max-w-60 rounded-xl border border-line object-cover" />
    </a>
  );
}

// the files on a user's message, above its text
export function MessageAttachments({ attachments }: { attachments: MessageAttachment[] }) {
  return (
    <div className="flex max-w-[85%] flex-wrap justify-end gap-2">
      {attachments.map((attachment, index) =>
        attachment.kind === "image" ? (
          <AttachedImage key={attachment.id ?? `${attachment.name}-${index}`} attachment={attachment} />
        ) : (
          <div key={attachment.id ?? `${attachment.name}-${index}`} className="flex h-12 max-w-56 items-center gap-2 rounded-xl border border-line bg-subtle/60 px-3">
            <LuFileText size={16} className="shrink-0 text-muted" />
            <span className="truncate text-xs">{attachment.name}</span>
          </div>
        ),
      )}
    </div>
  );
}
