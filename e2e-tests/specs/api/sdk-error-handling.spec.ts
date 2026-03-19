import { test, expect } from '@playwright/test';
import OpenAI from 'openai';
import Anthropic from '@anthropic-ai/sdk';
import { createOpenAIClient, createAnthropicClient, DEFAULT_MODEL, GATEWAY_URL } from '../../helpers/sdk-clients';

test.describe('SDK Error Handling', () => {
  // No retries — error tests should fail deterministically

  test('invalid API key throws AuthenticationError (OpenAI)', async () => {
    const badClient = new OpenAI({
      baseURL: `${GATEWAY_URL}/v1`,
      apiKey: 'invalid-key-that-does-not-exist',
    });

    await expect(
      badClient.chat.completions.create({
        model: DEFAULT_MODEL,
        messages: [{ role: 'user', content: 'hello' }],
        max_tokens: 10,
      })
    ).rejects.toThrow(OpenAI.AuthenticationError);
  });

  test('invalid API key throws AuthenticationError (Anthropic)', async () => {
    const badClient = new Anthropic({
      baseURL: GATEWAY_URL,
      apiKey: 'invalid-key-that-does-not-exist',
    });

    await expect(
      badClient.messages.create({
        model: DEFAULT_MODEL,
        messages: [{ role: 'user', content: 'hello' }],
        max_tokens: 10,
      })
    ).rejects.toThrow(Anthropic.AuthenticationError);
  });

  test('empty messages throws BadRequestError (OpenAI)', async () => {
    const client = createOpenAIClient();

    await expect(
      client.chat.completions.create({
        model: DEFAULT_MODEL,
        messages: [],
        max_tokens: 10,
      })
    ).rejects.toThrow(OpenAI.BadRequestError);
  });

  test('empty messages throws BadRequestError (Anthropic)', async () => {
    const client = createAnthropicClient();

    await expect(
      client.messages.create({
        model: DEFAULT_MODEL,
        messages: [],
        max_tokens: 10,
      })
    ).rejects.toThrow(Anthropic.BadRequestError);
  });

  test('invalid model throws error (OpenAI)', async () => {
    const client = createOpenAIClient();

    await expect(
      client.chat.completions.create({
        model: 'nonexistent-model-xyz',
        messages: [{ role: 'user', content: 'hello' }],
        max_tokens: 10,
      })
    ).rejects.toThrow();
  });

  test('malformed request via raw fetch', async ({ request }) => {
    const response = await request.post(`${GATEWAY_URL}/v1/chat/completions`, {
      headers: { 'Content-Type': 'application/json' },
      data: 'this is not json{{{',
    });

    expect(response.status()).toBe(400);
  });
});
