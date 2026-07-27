import { invoke, isTauri } from '@tauri-apps/api/core';
import type { ZodType } from 'zod';

type ContractSchemas = typeof import('../../../../../packages/contracts/src/schemas.ts');
type ContractSchemaName = keyof ContractSchemas;
type LazySchema<T> = () => Promise<ZodType<T>>;

export function contractSchema<T>(name: ContractSchemaName): LazySchema<T> {
  return async () => {
    const schemas = await import('../../../../../packages/contracts/src/schemas.ts');
    return schemas[name] as unknown as ZodType<T>;
  };
}

export async function invokeRequired<T, Args>(
  command: string,
  resultSchema: LazySchema<T>,
  argsSchema?: LazySchema<Args>,
  args?: Args
): Promise<T> {
  if (!isTauriRuntime()) {
    throw new Error('This command requires the Tauri desktop runtime.');
  }
  const commandArgs = await parseCommandArgs(command, argsSchema, args);
  const result = await invoke<unknown>(command, commandArgs);
  return parseCommandResult(command, resultSchema, result);
}

export async function invokeUnchecked(command: string, args?: Record<string, unknown>): Promise<unknown> {
  if (!isTauriRuntime()) {
    throw new Error('This command requires the Tauri desktop runtime.');
  }
  return invoke<unknown>(command, args);
}

export function withClientTimeout<T>(operation: Promise<T>, timeoutMs: number, message: string): Promise<T> {
  let timeoutId: ReturnType<typeof setTimeout>;
  const timeout = new Promise<T>((_, reject) => {
    timeoutId = setTimeout(() => reject(new Error(message)), timeoutMs);
  });
  operation.catch(() => undefined);
  return Promise.race([operation, timeout]).finally(() => clearTimeout(timeoutId));
}

function isTauriRuntime(): boolean {
  return isTauri();
}

async function parseCommandArgs<Args>(command: string, schema: LazySchema<Args> | undefined, args: Args | undefined) {
  if (!schema) return args as Record<string, unknown> | undefined;
  const parsed = (await schema()).safeParse(args);
  if (parsed.success) return parsed.data as Record<string, unknown>;
  throw new Error(formatContractError(command, 'args', parsed.error.issues));
}

async function parseCommandResult<T>(command: string, schema: LazySchema<T>, value: unknown): Promise<T> {
  const parsed = (await schema()).safeParse(value);
  if (parsed.success) return parsed.data;
  throw new Error(formatContractError(command, 'result', parsed.error.issues));
}

function formatContractError(
  command: string,
  boundary: 'args' | 'result',
  issues: Array<{ path: PropertyKey[]; message: string }>
) {
  const details = issues
    .slice(0, 6)
    .map((issue) => {
      const path = issue.path.length > 0 ? issue.path.join('.') : '$';
      return `${path}: ${issue.message}`;
    })
    .join('; ');
  return `${command} ${boundary} failed contract validation: ${details}`;
}
