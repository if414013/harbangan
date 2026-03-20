import { test, expect } from '@playwright/test';

test.describe('Anthropic messages (non-streaming)', () => {
  test.skip(!process.env.API_KEY, 'Requires API_KEY environment variable');

  test('POST /v1/messages returns valid response', async ({ request }) => {
    const response = await request.post('/v1/messages', {
      headers: {
        'anthropic-version': '2023-06-01',
      },
      data: {
        model: 'claude-sonnet-4-20250514',
        messages: [{ role: 'user', content: 'Say "hello" and nothing else.' }],
        max_tokens: 32,
      },
    });
    expect(response.status()).toBe(200);

    const body = await response.json();
    expect(body).toHaveProperty('content');
    expect(Array.isArray(body.content)).toBe(true);
    expect(body.content.length).toBeGreaterThan(0);
    expect(body).toHaveProperty('role', 'assistant');
    expect(body).toHaveProperty('usage');
    expect(body.usage).toHaveProperty('input_tokens');
    expect(body.usage).toHaveProperty('output_tokens');
  });
});
