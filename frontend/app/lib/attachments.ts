import { attachment } from "./api";

export const MAX_ATTACHMENTS = 10;
// the server takes base64 json bodies, so files stay well under what a request can carry
export const MAX_ATTACHMENT_BYTES = 20 * 1024 * 1024;
// the server's cap on a whole message, with base64 already counted
export const MAX_MESSAGE_CHARS = 60 * 1024 * 1024;
export const ATTACHMENT_TYPES = "image/png,image/jpeg,image/gif,image/webp,application/pdf,text/*,.md,.csv,.json,.ts,.tsx,.js,.jsx,.py,.rs,.go,.yaml,.yml,.toml";

export type AttachmentKind = "image" | "pdf" | "file";

// a file on a message: local ones carry their data url, saved ones their attachment id
export type MessageAttachment = { name: string; kind: AttachmentKind; url?: string; id?: string };

export const kindOf = (mime: string): AttachmentKind => (mime.startsWith("image/") ? "image" : mime === "application/pdf" ? "pdf" : "file");

export function readAsDataUrl(file: File): Promise<string> {
  return new Promise((resolve, reject) => {
    const reader = new FileReader();
    reader.onload = () => resolve(String(reader.result));
    reader.onerror = () => reject(reader.error ?? new Error(`could not read ${file.name}`));
    reader.readAsDataURL(file);
  });
}

// saved attachments are behind the bearer token, so they're fetched once and shown through object urls
const urls = new Map<string, Promise<string>>();

export function attachmentUrl(id: string): Promise<string> {
  let url = urls.get(id);
  if (!url) {
    url = attachment(id).then((blob) => URL.createObjectURL(blob));
    url.catch(() => urls.delete(id));
    urls.set(id, url);
  }
  return url;
}
