import { test, expect } from '@playwright/test';
import { createOpenAIClient, DEFAULT_MODEL } from '../../helpers/sdk-clients';

test.describe('OpenAI SDK Chat Completions', () => {
  test.describe.configure({ retries: 3 });

  const client = createOpenAIClient();

  test('basic chat completion', async () => {
    const response = await client.chat.completions.create({
      model: DEFAULT_MODEL,
      messages: [{ role: 'user', content: 'Say hello' }],
      max_tokens: 20,
    });

    expect(response.id).toBeTruthy();
    expect(response.choices).toHaveLength(1);
    expect(response.choices[0].message.content).toBeTruthy();
    expect(response.choices[0].message.role).toBe('assistant');
    expect(response.usage!.prompt_tokens).toBeGreaterThan(0);
    expect(response.usage!.completion_tokens).toBeGreaterThan(0);
  });

  test('with system message', async () => {
    const response = await client.chat.completions.create({
      model: DEFAULT_MODEL,
      messages: [
        { role: 'system', content: 'You are a helpful assistant.' },
        { role: 'user', content: 'Say hi' },
      ],
      max_tokens: 20,
    });

    expect(response.choices).toHaveLength(1);
    expect(response.choices[0].message.content).toBeTruthy();
  });

  test('multi-turn conversation', async () => {
    const response = await client.chat.completions.create({
      model: DEFAULT_MODEL,
      messages: [
        { role: 'user', content: 'My name is Alice' },
        { role: 'assistant', content: 'Hello Alice!' },
        { role: 'user', content: 'What did I just say?' },
      ],
      max_tokens: 30,
    });

    expect(response.choices).toHaveLength(1);
    expect(response.choices[0].message.content).toBeTruthy();
  });

  test('respects max_tokens', async () => {
    const response = await client.chat.completions.create({
      model: DEFAULT_MODEL,
      messages: [{ role: 'user', content: 'Count to 100' }],
      max_tokens: 5,
    });

    expect(response.usage!.completion_tokens).toBeLessThanOrEqual(10);
  });

  test('accepts temperature', async () => {
    const response = await client.chat.completions.create({
      model: DEFAULT_MODEL,
      messages: [{ role: 'user', content: 'Say one word' }],
      max_tokens: 10,
      temperature: 0,
    });

    expect(response.choices).toHaveLength(1);
    expect(response.choices[0].message.content).toBeTruthy();
  });
});
