import { test, expect } from '@playwright/test';
import { createOpenAIClient } from '../../helpers/sdk-clients';

test.describe('Models via SDK + Alias Resolution', () => {
  test.describe.configure({ retries: 3 });
  test.skip(!process.env.API_KEY, 'Requires API_KEY environment variable');

  const client = createOpenAIClient();

  test('list models via SDK', async () => {
    const models = await client.models.list();
    const data = [];
    for await (const model of models) {
      data.push(model);
    }
    expect(data.length).toBeGreaterThan(0);
  });

  test('model object structure', async () => {
    const models = await client.models.list();
    const data = [];
    for await (const model of models) {
      data.push(model);
    }

    for (const model of data) {
      expect(typeof model.id).toBe('string');
      expect(model.object).toBe('model');
      expect(typeof model.created).toBe('number');
      expect(typeof model.owned_by).toBe('string');
    }
  });

  test('dot-version alias resolves (claude-sonnet-4.6)', async () => {
    const response = await client.chat.completions.create({
      model: 'claude-sonnet-4.6',
      messages: [{ role: 'user', content: 'Say hi' }],
      max_tokens: 10,
    });

    expect(response.choices).toHaveLength(1);
    expect(response.choices[0].message.content).toBeTruthy();
  });

  test('dash-version alias resolves (claude-sonnet-4-6)', async () => {
    const response = await client.chat.completions.create({
      model: 'claude-sonnet-4-6',
      messages: [{ role: 'user', content: 'Say hi' }],
      max_tokens: 10,
    });

    expect(response.choices).toHaveLength(1);
    expect(response.choices[0].message.content).toBeTruthy();
  });

  test('date-suffix alias resolves to base model (claude-sonnet-4-20250514)', async () => {
    // This resolves to claude-sonnet-4, NOT claude-sonnet-4.6
    const response = await client.chat.completions.create({
      model: 'claude-sonnet-4-20250514',
      messages: [{ role: 'user', content: 'Say hi' }],
      max_tokens: 10,
    });

    expect(response.choices).toHaveLength(1);
    expect(response.choices[0].message.content).toBeTruthy();
  });
});
