import { test, expect } from '@playwright/test';
import type { MessageStreamEvent } from '@anthropic-ai/sdk/resources/messages';
import { createAnthropicClient, DEFAULT_MODEL } from '../../helpers/sdk-clients';

test.describe('Anthropic SDK Streaming', () => {
  test.describe.configure({ retries: 3 });

  const client = createAnthropicClient();

  test('streaming collects text', async () => {
    const stream = client.messages.stream({
      model: DEFAULT_MODEL,
      messages: [{ role: 'user', content: 'Say hello' }],
      max_tokens: 20,
    });

    let fullText = '';
    for await (const event of stream) {
      if (event.type === 'content_block_delta' && event.delta.type === 'text_delta') {
        fullText += event.delta.text;
      }
    }

    expect(fullText.length).toBeGreaterThan(0);
  });

  test('stream events have correct invariants', async () => {
    const stream = client.messages.stream({
      model: DEFAULT_MODEL,
      messages: [{ role: 'user', content: 'Say hi' }],
      max_tokens: 20,
    });

    const events: MessageStreamEvent[] = [];
    stream.on('streamEvent', (event) => {
      events.push(event);
    });

    // Consume the stream to completion
    await stream.finalMessage();

    expect(events.length).toBeGreaterThan(0);

    // First event is message_start
    expect(events[0].type).toBe('message_start');

    // Last event is message_stop
    expect(events[events.length - 1].type).toBe('message_stop');

    // message_delta appears before message_stop
    const messageDeltaIdx = events.findIndex((e) => e.type === 'message_delta');
    const messageStopIdx = events.findIndex((e) => e.type === 'message_stop');
    expect(messageDeltaIdx).toBeGreaterThan(-1);
    expect(messageDeltaIdx).toBeLessThan(messageStopIdx);

    // Every content_block_start has a matching content_block_stop
    const blockStarts = events.filter((e) => e.type === 'content_block_start');
    const blockStops = events.filter((e) => e.type === 'content_block_stop');
    expect(blockStarts.length).toBe(blockStops.length);
    for (let i = 0; i < blockStarts.length; i++) {
      const startIdx = events.indexOf(blockStarts[i]);
      const stopIdx = events.indexOf(blockStops[i]);
      expect(stopIdx).toBeGreaterThan(startIdx);
    }

    // At least one text_delta exists
    const hasTextDelta = events.some(
      (e) =>
        e.type === 'content_block_delta' && e.delta.type === 'text_delta'
    );
    expect(hasTextDelta).toBe(true);
  });

  test('final message has usage', async () => {
    const stream = client.messages.stream({
      model: DEFAULT_MODEL,
      messages: [{ role: 'user', content: 'Say one word' }],
      max_tokens: 10,
    });

    const events: MessageStreamEvent[] = [];
    stream.on('streamEvent', (event) => {
      events.push(event);
    });

    await stream.finalMessage();

    const messageDelta = events.find((e) => e.type === 'message_delta');
    expect(messageDelta).toBeTruthy();
    expect(messageDelta!.type).toBe('message_delta');
    if (messageDelta!.type === 'message_delta') {
      expect(messageDelta!.usage.output_tokens).toBeGreaterThan(0);
    }
  });
});
