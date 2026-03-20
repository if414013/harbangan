import { test, expect } from '@playwright/test';
import type { MessageStreamEvent } from '@anthropic-ai/sdk/resources/messages';
import { createOpenAIClient, createAnthropicClient, DEFAULT_MODEL } from '../../helpers/sdk-clients';

test.describe('Thinking/Reasoning Parameter Acceptance', () => {
  test.describe.configure({ retries: 3 });
  test.skip(!process.env.API_KEY, 'Requires API_KEY environment variable');

  test('accepts reasoning_effort parameter (OpenAI)', async () => {
    const client = createOpenAIClient();

    // reasoning_effort is silently dropped by the proxy — validate no error
    const response = await client.chat.completions.create({
      model: DEFAULT_MODEL,
      messages: [{ role: 'user', content: 'Say hi' }],
      max_tokens: 50,
      reasoning_effort: 'low',
    });

    expect(response.choices).toHaveLength(1);
    expect(response.choices[0].message.content).toBeTruthy();
  });

  test('accepts thinking parameter (Anthropic)', async () => {
    const client = createAnthropicClient();

    // thinking parameter is silently dropped — validate no error
    const response = await client.messages.create({
      model: DEFAULT_MODEL,
      messages: [{ role: 'user', content: 'Say hi' }],
      max_tokens: 50,
      thinking: { type: 'enabled', budget_tokens: 1024 },
    });

    expect(response.content.length).toBeGreaterThan(0);
    expect(response.role).toBe('assistant');
  });

  test('accepts thinking in streaming (Anthropic)', async () => {
    const client = createAnthropicClient();

    const stream = client.messages.stream({
      model: DEFAULT_MODEL,
      messages: [{ role: 'user', content: 'Say hello' }],
      max_tokens: 50,
      thinking: { type: 'enabled', budget_tokens: 1024 },
    });

    const events: MessageStreamEvent[] = [];
    stream.on('streamEvent', (event) => {
      events.push(event);
    });

    let fullText = '';
    for await (const event of stream) {
      if (event.type === 'content_block_delta' && event.delta.type === 'text_delta') {
        fullText += event.delta.text;
      }
    }

    // Stream completed with message_stop
    expect(events.some((e) => e.type === 'message_stop')).toBe(true);
    expect(fullText.length).toBeGreaterThan(0);
  });
});
