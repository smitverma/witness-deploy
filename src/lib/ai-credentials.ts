import { commands } from "./api";
import type { AiKeyStatus } from "./types";

export function displayAiKey(prefix: string, suffix: string): string {
  return `${prefix}••••••${suffix}`;
}

export async function saveAiApiKey(value: string): Promise<AiKeyStatus> {
  if (!value) throw new Error("API key cannot be empty");
  return commands.setAiApiKey(value);
}

export async function deleteAiApiKey(): Promise<AiKeyStatus> {
  return commands.deleteAiApiKey();
}

export async function waitForAiKeyOperation(initial: AiKeyStatus): Promise<AiKeyStatus> {
  let status = initial;
  const deadline = Date.now() + 180_000;
  while (status.pending) {
    if (Date.now() >= deadline) {
      throw new Error("The secure key operation is taking too long. Check the application logs and try again.");
    }
    await new Promise((resolve) => window.setTimeout(resolve, 500));
    status = await commands.getAiApiKeyStatus();
  }
  if (status.error) throw new Error(status.error);
  return status;
}
