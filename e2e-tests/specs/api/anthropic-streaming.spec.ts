import { test, expect } from '@playwright/test';

test.describe('Anthropic messages (streaming)', () => {
  test.skip(!process.env.API_KEY, 'Requires API_KEY environment variable');

  test('POST /v1/messages with stream:true returns SSE events', async ({ request }) => {
    const response = await request.post('/v1/messages', {
      headers: {
        'anthropic-version': '2023-06-01',
      },
      data: {
        model: 'claude-sonnet-4-20250514',
        messages: [{ role: 'user', content: 'Say "hello" and nothing else.' }],
        max_tokens: 32,
        stream: true,
      },
    });
    expect(response.status()).toBe(200);

    const contentType = response.headers()['content-type'] || '';
    expect(contentType).toContain('text/event-stream');

    const text = await response.text();
    const eventTypes = text
      .split('\n')
      .filter((l) => l.startsWith('event: '))
      .map((l) => l.replace('event: ', '').trim());

    expect(eventTypes).toContain('message_start');
    expect(eventTypes).toContain('content_block_delta');
    expect(eventTypes).toContain('message_stop');
  });
});
