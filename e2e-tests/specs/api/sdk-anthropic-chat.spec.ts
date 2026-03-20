import { test, expect } from '@playwright/test';
import { createAnthropicClient, DEFAULT_MODEL } from '../../helpers/sdk-clients';

test.describe('Anthropic SDK Chat Messages', () => {
  test.describe.configure({ retries: 3 });
  test.skip(!process.env.API_KEY, 'Requires API_KEY environment variable');

  const client = createAnthropicClient();

  test('basic message', async () => {
    const response = await client.messages.create({
      model: DEFAULT_MODEL,
      messages: [{ role: 'user', content: 'Say hello' }],
      max_tokens: 20,
    });

    expect(response.id).toBeTruthy();
    expect(response.type).toBe('message');
    expect(response.role).toBe('assistant');
    expect(response.content).toHaveLength(1);
    expect(response.content[0].type).toBe('text');
    expect((response.content[0] as { type: 'text'; text: string }).text).toBeTruthy();
    expect(response.usage.input_tokens).toBeGreaterThan(0);
    expect(response.usage.output_tokens).toBeGreaterThan(0);
  });

  test('with system prompt', async () => {
    const response = await client.messages.create({
      model: DEFAULT_MODEL,
      messages: [{ role: 'user', content: 'Say hi' }],
      system: 'You are a helpful assistant.',
      max_tokens: 20,
    });

    expect(response.content).toHaveLength(1);
    expect((response.content[0] as { type: 'text'; text: string }).text).toBeTruthy();
  });

  test('multi-turn conversation', async () => {
    const response = await client.messages.create({
      model: DEFAULT_MODEL,
      messages: [
        { role: 'user', content: 'My name is Alice' },
        { role: 'assistant', content: 'Hello Alice!' },
        { role: 'user', content: 'What did I just say?' },
      ],
      max_tokens: 30,
    });

    expect(response.content).toHaveLength(1);
    expect((response.content[0] as { type: 'text'; text: string }).text).toBeTruthy();
  });

  test('respects max_tokens', async () => {
    const response = await client.messages.create({
      model: DEFAULT_MODEL,
      messages: [{ role: 'user', content: 'Count to 100' }],
      max_tokens: 5,
    });

    expect(response.usage.output_tokens).toBeLessThanOrEqual(10);
  });

  test('stop_reason is valid', async () => {
    const response = await client.messages.create({
      model: DEFAULT_MODEL,
      messages: [{ role: 'user', content: 'Say one word' }],
      max_tokens: 50,
    });

    expect(['end_turn', 'max_tokens']).toContain(response.stop_reason);
  });
});
