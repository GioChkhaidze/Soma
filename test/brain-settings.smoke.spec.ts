import { expect, test, type Page } from '@playwright/test';

const storedProviderSettings = {
  providerId: 'openai',
  model: 'gpt-5.5',
  endpoint: 'https://api.openai.com/v1',
  authProfile: '',
  credentialConfigured: true,
  updatedAt: '2026-07-25T00:00:00.000Z'
};

const storedCodexSettings = {
  providerId: 'codex_sdk',
  model: 'gpt-5.6-luna',
  endpoint: '',
  authProfile: '',
  credentialConfigured: false,
  updatedAt: '2026-07-25T00:00:00.000Z',
  effectiveModel: 'gpt-5.6-luna',
  modelSource: 'selected',
  defaultReasoningEffort: 'medium',
  graphReasoningEffort: 'xhigh'
};

test('provider family follows settings that finish loading after the panel opens', async ({ page }) => {
  await installDelayedSettingsMock(page);
  await openSettings(page);

  await expect(familyButton(page, 'Coding Agents')).toHaveAttribute('aria-pressed', 'true');
  await resolveSettingsLoad(page);
  await expect(familyButton(page, 'Providers')).toHaveAttribute('aria-pressed', 'true');
  await expect(page.getByLabel('Selected brain')).toContainText('OpenAI');
  await expect(page.getByText('Keep compile folders')).toHaveCount(0);
});

test('delayed settings do not replace a provider family chosen while loading', async ({ page }) => {
  await installDelayedSettingsMock(page);
  await openSettings(page);

  await familyButton(page, 'Local').click();
  await expect(page.getByLabel('Selected brain')).toContainText('Ollama');
  await resolveSettingsLoad(page);
  await waitForSettingsDelivery(page);
  await page.waitForTimeout(100);

  await expect(familyButton(page, 'Local')).toHaveAttribute('aria-pressed', 'true');
  await expect(page.getByLabel('Selected brain')).toContainText('Ollama');
});

test('Codex model and efforts save as one active Brain policy', async ({ page }) => {
  await installCodexPolicyMock(page);
  await openSettings(page);

  await page.getByRole('radio', { name: 'GPT-5.6 Terra' }).click();
  await page.getByLabel('Chat effort').selectOption('low');
  await page.getByLabel('Graph effort').selectOption('max');

  await expect(page.getByLabel('Selected brain'))
    .toContainText(/Draft: gpt-5\.6-terra .* chat low .* graph max/);

  await page.locator('.aiSettingsFooter button').click();
  await expect.poll(() => savedBrainPolicy(page)).toMatchObject({
    model: 'gpt-5.6-terra',
    defaultReasoningEffort: 'low',
    graphReasoningEffort: 'max'
  });
  await expect(page.getByLabel('Selected brain'))
    .toContainText(/Active: gpt-5\.6-terra .* chat low .* graph max/);
});

test('one save owns the request and preserves a secret typed while it finishes', async ({ page }) => {
  await installDelayedSaveMock(page);
  await openSettings(page);

  const secretInput = page.getByLabel('OpenAI API key');
  const saveButton = page.locator('.aiSettingsFooter button');
  await secretInput.fill('first-test-key');
  await saveButton.evaluate((button) => {
    button.click();
    button.click();
  });

  await expect.poll(() => saveCount(page)).toBe(1);
  await expect(saveButton).toBeDisabled();
  await page.getByRole('option', { name: 'GPT-5.4 gpt-5.4 Default', exact: true }).click();
  await secretInput.fill('newer-test-key');
  await resolveSettingsSave(page);

  await expect(page.getByText('Brain settings saved. Newer edits are not saved yet.', { exact: true })).toBeVisible();
  await expect(page.getByLabel('Selected brain')).toContainText('gpt-5.4');
  await expect(secretInput).toHaveValue('newer-test-key');
  await expect(saveButton).toBeEnabled();
  await expect.poll(() => saveCount(page)).toBe(1);
});

test('Claude Code uses its active local login without a profile field', async ({ page }) => {
  await installEndpointAuthorityMock(page);
  await openSettings(page);

  await familyButton(page, 'Coding Agents').click();
  await page.getByRole('radio', { name: /Claude Code/ }).click();

  await expect(page.getByLabel('Selected brain')).toContainText('Claude Code');
  await expect(page.getByText('Use the installed Claude Code CLI with its active local login.')).toBeVisible();
  await expect(page.getByLabel('Codex auth')).toHaveCount(0);
  await expect(page.getByLabel('Claude Code model')).toHaveAttribute(
    'placeholder',
    'Claude Code model alias (optional)'
  );
});

test('Vercel defaults stay placeholders while list and save send a blank override', async ({ page }) => {
  await installEndpointAuthorityMock(page);
  await openSettings(page);

  await page.getByRole('radio', { name: /Vercel Gateway/ }).click();
  const baseUrl = page.getByLabel('Base URL');
  await expect(baseUrl).toHaveValue('');
  await expect(baseUrl).toHaveAttribute('placeholder', 'https://ai-gateway.vercel.sh/v1');

  await page.getByRole('button', { name: 'Refresh' }).click();
  await expect.poll(() => capturedEndpoint(page, '__listedEndpoint')).toBe('');

  await page.locator('.aiSettingsFooter button').click();
  await expect.poll(() => capturedEndpoint(page, '__savedEndpoint')).toBe('');
});

async function openSettings(page: Page) {
  await page.goto('/');
  await page.getByRole('button', { name: 'Settings' }).click();
  await expect(page.getByLabel('Settings detail')).toBeVisible();
}

function familyButton(page: Page, title: string) {
  return page.getByRole('button', { name: new RegExp(`^${title}\\b`) });
}

async function installDelayedSettingsMock(page: Page) {
  await page.addInitScript((settings) => {
    const state = globalThis as typeof globalThis & SettingsTestState;
    let resolveSettings: ((value: typeof settings) => void) | null = null;
    const settingsPromise = new Promise<typeof settings>((resolve) => {
      resolveSettings = resolve;
    });

    state.isTauri = true;
    state.__resolveBrainSettings = () => {
      resolveSettings?.(settings);
    };
    state.__TAURI_INTERNALS__ = {
      invoke: async (command) => {
        if (command === 'get_current_workspace') {
          return {
            has_workspace: false,
            workspace_dir: null,
            database_path: null
          };
        }
        if (command === 'get_brain_settings') {
          const loaded = await settingsPromise;
          state.__brainSettingsDelivered = true;
          return loaded;
        }
        throw new Error(`Unexpected Tauri command in settings smoke test: ${command}`);
      }
    };
  }, storedProviderSettings);
}

async function installDelayedSaveMock(page: Page) {
  await page.addInitScript((settings) => {
    const state = globalThis as typeof globalThis & SettingsTestState;
    let resolveSave: ((value: typeof settings) => void) | null = null;

    state.isTauri = true;
    state.__brainSaveCount = 0;
    state.__resolveBrainSave = () => {
      resolveSave?.(settings);
    };
    state.__TAURI_INTERNALS__ = {
      invoke: async (command) => {
        if (command === 'get_current_workspace') {
          return {
            has_workspace: false,
            workspace_dir: null,
            database_path: null
          };
        }
        if (command === 'get_brain_settings') return settings;
        if (command === 'save_brain_settings') {
          state.__brainSaveCount = (state.__brainSaveCount ?? 0) + 1;
          return new Promise<typeof settings>((resolve) => {
            resolveSave = resolve;
          });
        }
        throw new Error(`Unexpected Tauri command in settings smoke test: ${command}`);
      }
    };
  }, storedProviderSettings);
}

async function installCodexPolicyMock(page: Page) {
  await page.addInitScript((settings) => {
    const state = globalThis as typeof globalThis & SettingsTestState;
    state.isTauri = true;
    state.__TAURI_INTERNALS__ = {
      invoke: async (command, args) => {
        if (command === 'get_current_workspace') {
          return {
            has_workspace: false,
            workspace_dir: null,
            database_path: null
          };
        }
        if (command === 'get_brain_settings') return settings;
        const payload = args?.settings ?? {};
        if (command === 'save_brain_settings') {
          state.__savedBrainPolicy = payload;
          return {
            ...settings,
            ...payload,
            effectiveModel: String(payload.model ?? settings.effectiveModel),
            modelSource: 'selected',
            credentialConfigured: false,
            updatedAt: '2026-07-25T01:00:00.000Z'
          };
        }
        throw new Error(`Unexpected Tauri command in Codex policy test: ${command}`);
      }
    };
  }, storedCodexSettings);
}

async function installEndpointAuthorityMock(page: Page) {
  await page.addInitScript((settings) => {
    const state = globalThis as typeof globalThis & SettingsTestState;
    state.isTauri = true;
    state.__TAURI_INTERNALS__ = {
      invoke: async (command, args) => {
        if (command === 'get_current_workspace') {
          return {
            has_workspace: false,
            workspace_dir: null,
            database_path: null
          };
        }
        if (command === 'get_brain_settings') return settings;
        const payload = args?.settings as Record<string, unknown> | undefined;
        if (command === 'list_brain_models') {
          state.__listedEndpoint = payload?.endpoint;
          return {
            providerId: String(payload?.providerId ?? ''),
            status: 'ready',
            message: 'Models loaded.',
            models: []
          };
        }
        if (command === 'save_brain_settings') {
          state.__savedEndpoint = payload?.endpoint;
          return {
            providerId: String(payload?.providerId ?? settings.providerId),
            model: String(payload?.model ?? ''),
            endpoint: String(payload?.endpoint ?? ''),
            authProfile: String(payload?.authProfile ?? ''),
            credentialConfigured: false,
            updatedAt: '2026-07-25T01:00:00.000Z'
          };
        }
        throw new Error(`Unexpected Tauri command in endpoint authority test: ${command}`);
      }
    };
  }, storedProviderSettings);
}

async function resolveSettingsLoad(page: Page) {
  await page.evaluate(() => {
    (globalThis as typeof globalThis & SettingsTestState).__resolveBrainSettings?.();
  });
}

async function waitForSettingsDelivery(page: Page) {
  await expect.poll(() => page.evaluate(
    () => (globalThis as typeof globalThis & SettingsTestState).__brainSettingsDelivered === true
  )).toBe(true);
}

async function resolveSettingsSave(page: Page) {
  await page.evaluate(() => {
    (globalThis as typeof globalThis & SettingsTestState).__resolveBrainSave?.();
  });
}

async function saveCount(page: Page) {
  return page.evaluate(() => (
    (globalThis as typeof globalThis & SettingsTestState).__brainSaveCount ?? 0
  ));
}

async function savedBrainPolicy(page: Page) {
  return page.evaluate(() => (
    (globalThis as typeof globalThis & SettingsTestState).__savedBrainPolicy ?? {}
  ));
}

async function capturedEndpoint(page: Page, key: '__listedEndpoint' | '__savedEndpoint') {
  return page.evaluate((field) => (
    (globalThis as typeof globalThis & SettingsTestState)[field]
  ), key);
}

type SettingsTestState = {
  isTauri: boolean;
  __TAURI_INTERNALS__: {
    invoke: (command: string, args?: { settings?: Record<string, unknown> }) => Promise<unknown>;
  };
  __resolveBrainSettings?: () => void;
  __brainSettingsDelivered?: boolean;
  __brainSaveCount?: number;
  __resolveBrainSave?: () => void;
  __savedBrainPolicy?: Record<string, unknown>;
  __listedEndpoint?: unknown;
  __savedEndpoint?: unknown;
};
