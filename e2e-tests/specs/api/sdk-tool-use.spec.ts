import { test, expect } from '@playwright/test';
import { createOpenAIClient, createAnthropicClient, DEFAULT_MODEL } from '../../helpers/sdk-clients';

const weatherTool = {
  type: 'function' as const,
  function: {
    name: 'get_weather',
    description: 'Get the current weather for a location',
    parameters: {
      type: 'object',
      properties: {
        location: { type: 'string', description: 'City name' },
      },
      required: ['location'],
    },
  },
};

const anthropicWeatherTool = {
  name: 'get_weather',
  description: 'Get the current weather for a location',
  input_schema: {
    type: 'object' as const,
    properties: {
      location: { type: 'string', description: 'City name' },
    },
    required: ['location'],
  },
};

test.describe('Tool Use Acceptance', () => {
  test.describe.configure({ retries: 3 });

  test('accepts tools array (OpenAI SDK)', async () => {
    const client = createOpenAIClient();

    const response = await client.chat.completions.create({
      model: DEFAULT_MODEL,
      messages: [{ role: 'user', content: 'What is the weather in Tokyo?' }],
      tools: [weatherTool],
      max_tokens: 200,
    });

    expect(response.choices).toHaveLength(1);
    const choice = response.choices[0];

    // Either the model called the tool or responded with text — both are valid
    if (choice.finish_reason === 'tool_calls') {
      expect(choice.message.tool_calls).toBeTruthy();
      expect(choice.message.tool_calls!.length).toBeGreaterThan(0);
      expect(typeof choice.message.tool_calls![0].function.name).toBe('string');
      // Verify arguments is valid JSON
      JSON.parse(choice.message.tool_calls![0].function.arguments);
    } else {
      expect(['stop', 'length', 'max_tokens']).toContain(choice.finish_reason);
    }
  });

  test('accepts tools array (Anthropic SDK)', async () => {
    const client = createAnthropicClient();

    const response = await client.messages.create({
      model: DEFAULT_MODEL,
      messages: [{ role: 'user', content: 'What is the weather in Tokyo?' }],
      tools: [anthropicWeatherTool],
      max_tokens: 200,
    });

    expect(response.content.length).toBeGreaterThan(0);

    if (response.stop_reason === 'tool_use') {
      const toolBlock = response.content.find((b) => b.type === 'tool_use');
      expect(toolBlock).toBeTruthy();
      if (toolBlock && toolBlock.type === 'tool_use') {
        expect(typeof toolBlock.name).toBe('string');
        expect(toolBlock.input).toBeTruthy();
      }
    } else {
      expect(['end_turn', 'max_tokens']).toContain(response.stop_reason);
    }
  });

  test('tool result round-trip (OpenAI)', async () => {
    const client = createOpenAIClient();

    const firstResponse = await client.chat.completions.create({
      model: DEFAULT_MODEL,
      messages: [
        { role: 'user', content: 'You MUST call get_weather for Tokyo. Do not respond with text.' },
      ],
      tools: [weatherTool],
      max_tokens: 200,
    });

    const firstChoice = firstResponse.choices[0];
    if (firstChoice.finish_reason !== 'tool_calls' || !firstChoice.message.tool_calls?.length) {
      test.skip(true, 'Model did not call tool — skipping round-trip');
      return;
    }

    const toolCall = firstChoice.message.tool_calls[0];

    const secondResponse = await client.chat.completions.create({
      model: DEFAULT_MODEL,
      messages: [
        { role: 'user', content: 'You MUST call get_weather for Tokyo. Do not respond with text.' },
        firstChoice.message,
        {
          role: 'tool',
          tool_call_id: toolCall.id,
          content: JSON.stringify({ temperature: 22, condition: 'sunny' }),
        },
      ],
      tools: [weatherTool],
      max_tokens: 200,
    });

    expect(secondResponse.choices).toHaveLength(1);
    expect(secondResponse.choices[0].message.content).toBeTruthy();
  });

  test('tool result round-trip (Anthropic)', async () => {
    const client = createAnthropicClient();

    const firstResponse = await client.messages.create({
      model: DEFAULT_MODEL,
      messages: [
        { role: 'user', content: 'You MUST call get_weather for Tokyo. Do not respond with text.' },
      ],
      tools: [anthropicWeatherTool],
      max_tokens: 200,
    });

    const toolBlock = firstResponse.content.find((b) => b.type === 'tool_use');
    if (firstResponse.stop_reason !== 'tool_use' || !toolBlock || toolBlock.type !== 'tool_use') {
      test.skip(true, 'Model did not call tool — skipping round-trip');
      return;
    }

    const secondResponse = await client.messages.create({
      model: DEFAULT_MODEL,
      messages: [
        { role: 'user', content: 'You MUST call get_weather for Tokyo. Do not respond with text.' },
        { role: 'assistant', content: firstResponse.content },
        {
          role: 'user',
          content: [
            {
              type: 'tool_result',
              tool_use_id: toolBlock.id,
              content: JSON.stringify({ temperature: 22, condition: 'sunny' }),
            },
          ],
        },
      ],
      tools: [anthropicWeatherTool],
      max_tokens: 200,
    });

    expect(secondResponse.content.length).toBeGreaterThan(0);
    const textBlock = secondResponse.content.find((b) => b.type === 'text');
    expect(textBlock).toBeTruthy();
  });
});
