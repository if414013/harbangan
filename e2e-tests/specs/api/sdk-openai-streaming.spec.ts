import { test, expect } from '@playwright/test';
import { createOpenAIClient, DEFAULT_MODEL } from '../../helpers/sdk-clients';

test.describe('OpenAI SDK Streaming', () => {
  test.describe.configure({ retries: 3 });

  const client = createOpenAIClient();

  test('streaming returns chunks', async () => {
    const stream = await client.chat.completions.create({
      model: DEFAULT_MODEL,
      messages: [{ role: 'user', content: 'Say hello' }],
      max_tokens: 20,
      stream: true,
    });

    const chunks = [];
    for await (const chunk of stream) {
      chunks.push(chunk);
    }

    expect(chunks.length).toBeGreaterThan(0);
    const hasContent = chunks.some(
      (c) => c.choices[0]?.delta?.content != null
    );
    expect(hasContent).toBe(true);
  });

  test('first chunk has role', async () => {
    const stream = await client.chat.completions.create({
      model: DEFAULT_MODEL,
      messages: [{ role: 'user', content: 'Say hi' }],
      max_tokens: 10,
      stream: true,
    });

    const chunks = [];
    for await (const chunk of stream) {
      chunks.push(chunk);
    }

    expect(chunks[0].choices[0].delta.role).toBe('assistant');
  });

  test('last chunk has finish_reason', async () => {
    const stream = await client.chat.completions.create({
      model: DEFAULT_MODEL,
      messages: [{ role: 'user', content: 'Say one word' }],
      max_tokens: 10,
      stream: true,
    });

    const chunks = [];
    for await (const chunk of stream) {
      chunks.push(chunk);
    }

    const lastWithReason = chunks.find(
      (c) => c.choices[0]?.finish_reason != null
    );
    expect(lastWithReason).toBeTruthy();
    expect(['stop', 'length', 'max_tokens']).toContain(
      lastWithReason!.choices[0].finish_reason
    );
  });

  test('concatenated deltas form text', async () => {
    const stream = await client.chat.completions.create({
      model: DEFAULT_MODEL,
      messages: [{ role: 'user', content: 'Say hello world' }],
      max_tokens: 20,
      stream: true,
    });

    let fullText = '';
    for await (const chunk of stream) {
      const content = chunk.choices[0]?.delta?.content;
      if (content) {
        fullText += content;
      }
    }

    expect(fullText.length).toBeGreaterThan(0);
  });
});
