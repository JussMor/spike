/**
 * example.js — OpenAI Codex / GPT-4o agent using vcs tools
 *
 * Run:  OPENAI_API_KEY=sk-... node example.js
 */

import OpenAI from 'openai'
import { vcsTools, vcsSystemPrompt, handleVcsTool } from './tools.js'

const openai = new OpenAI()

async function runVcsAgent(task) {
  console.log(`\nTask: ${task}\n${'─'.repeat(60)}`)

  const messages = [
    { role: 'system', content: vcsSystemPrompt },
    { role: 'user', content: task },
  ]

  // Agentic loop — runs until no more tool calls
  while (true) {
    const response = await openai.chat.completions.create({
      model: 'gpt-4o',
      messages,
      tools: vcsTools,
      tool_choice: 'auto',
    })

    const choice = response.choices[0]
    messages.push(choice.message)

    if (!choice.message.tool_calls?.length) {
      // Model finished — no more tool calls
      console.log('\nAgent response:', choice.message.content)
      break
    }

    // Execute each tool call
    for (const toolCall of choice.message.tool_calls) {
      const name = toolCall.function.name
      const args = JSON.parse(toolCall.function.arguments)

      console.log(`  → ${name}(${JSON.stringify(args)})`)

      let result
      try {
        result = await handleVcsTool(name, args)
        console.log(`    ✓`, JSON.stringify(result))
      } catch (e) {
        result = { error: e.message }
        console.log(`    ✗`, e.message)
      }

      messages.push({
        role: 'tool',
        tool_call_id: toolCall.id,
        content: JSON.stringify(result),
      })
    }
  }
}

// ── Example task ──────────────────────────────────────────────────────────

await runVcsAgent(
  'Add a LoginForm React component to src/components/LoginForm.tsx with ' +
  'email and password inputs. Include data-testid attributes on all interactive ' +
  'elements. Track all changes through vcs.',
)
