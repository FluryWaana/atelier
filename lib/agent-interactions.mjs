import { randomUUID } from 'node:crypto';

// Keep original tool inputs for replies; the transport redacts their displayed copies.
export function createAgentProtocol({ type, mode, repo, prompt, model, effort, apiKey = '', send, emit, complete, waiting, protect = () => {} }) {
  const requests = new Map(), items = new Map();
  let usage, threadId, initialized = false;
  const publish = () => {
    waiting([...requests.values()].some(entry => entry.blocking));
    emit({ type: 'interaction', requests: [...requests.values()].map(entry => entry.view) });
  };
  function remove(wireId) {
    for (const [id, entry] of requests) if (entry.wireId === wireId) { requests.delete(id); publish(); break; }
  }
  function ask(wireId, view, respond, blocking = true) {
    if ([...requests.values()].some(entry => entry.wireId === wireId)) return;
    const id = randomUUID();
    requests.set(id, { wireId, view: { id, ...view }, respond, blocking });
    publish();
  }
  const questions = list => list.map((q, index) => ({ id: String(index), question: q.question, options: q.options || [], multiSelect: q.multiSelect === true, isSecret: q.isSecret === true }));
  const rpc = (id, method, params) => send({ id, method, params });
  const startThread = () => rpc('thread', 'thread/start', { cwd: repo, ...(model ? { model } : {}), approvalPolicy: 'on-request', approvalsReviewer: 'user', sandbox: mode === 'edit' ? 'workspace-write' : 'read-only', config: { 'features.default_mode_request_user_input': true } });
  const claudeResponse = (id, response) => send({ type: 'control_response', response: { subtype: 'success', request_id: id, response } });
  return {
    start() {
      if (type === 'codex') rpc('initialize', 'initialize', { clientInfo: { name: 'atelier_kanban', title: 'Atelier Kanban', version: '1.0.0' }, capabilities: { experimentalApi: true } });
      else send({ type: 'control_request', request_id: 'initialize', request: { subtype: 'initialize' } });
    },
    reply(id, answer) {
      const entry = requests.get(id);
      if (!entry) throw new Error('Cette demande n’est plus en attente.');
      for (const question of entry.view.questions || []) if (question.isSecret && answer.answers?.[question.id]) protect(answer.answers[question.id]);
      entry.respond(answer);
      requests.delete(id); publish();
    },
    receive(event) {
      if (type === 'claude') {
        if (event.type === 'control_response' && event.response?.request_id === 'initialize' && !initialized) {
          if (event.response.subtype === 'error') throw new Error(event.response.error || 'Initialisation Claude impossible.');
          initialized = true;
          send({ type: 'user', session_id: '', parent_tool_use_id: null, message: { role: 'user', content: prompt } });
        } else if (event.type === 'control_cancel_request') remove(event.request_id);
        else if (event.type === 'control_request') {
          const request = event.request || {}, input = request.input || {};
          if (request.subtype !== 'can_use_tool') {
            send({ type: 'control_response', response: { subtype: 'error', request_id: event.request_id, error: 'Interaction non prise en charge par Atelier.' } });
            emit({ type: 'system', subtype: 'interaction_unsupported', message: 'Interaction non prise en charge : ' + request.subtype });
            return;
          }
          const isQuestion = request.tool_name === 'AskUserQuestion' && Array.isArray(input.questions) && input.questions.length > 0;
          ask(event.request_id, isQuestion ? { kind: 'question', title: 'L’agent attend votre réponse', questions: questions(input.questions) }
            : { kind: 'approval', title: request.tool_name || 'Autorisation demandée', detail: JSON.stringify({ input, reason: request.decision_reason }, null, 2), canAllow: !(mode === 'analyze' && request.tool_name === 'ExitPlanMode') }, answer => {
            if (answer.decision === 'deny') return claudeResponse(event.request_id, { behavior: 'deny', message: 'Demande refusée par l’utilisateur.' });
            const updatedInput = isQuestion ? { ...input, answers: Object.fromEntries(input.questions.map((q, index) => [q.question, answer.answers[String(index)]])) } : input;
            claudeResponse(event.request_id, { behavior: 'allow', updatedInput });
          });
        } else {
          emit(event);
          if (event.type === 'result') complete(event.is_error ? 1 : 0);
        }
        return;
      }
      const p = event.params || {};
      if (event.method && event.id !== undefined) {
        const respond = result => send({ id: event.id, result });
        if (event.method === 'item/tool/requestUserInput' && Array.isArray(p.questions) && p.questions.length) {
          ask(event.id, { kind: 'question', title: 'L’agent attend votre réponse', questions: questions(p.questions) }, answer => respond({ answers: Object.fromEntries(p.questions.map((q, index) => [q.id, { answers: answer.decision === 'deny' ? [] : answer.answers[String(index)] }])) }), p.isBlocking !== false);
        } else if (['item/commandExecution/requestApproval', 'item/fileChange/requestApproval', 'item/permissions/requestApproval'].includes(event.method)) {
          const permissions = event.method === 'item/permissions/requestApproval';
          const item = items.get(JSON.stringify([p.threadId, p.itemId]));
          const hasDetails = event.method !== 'item/fileChange/requestApproval' || Array.isArray(item?.changes) && item.changes.length > 0;
          ask(event.id, { kind: 'approval', title: p.reason || 'Autorisation demandée', detail: JSON.stringify({ ...item, ...p }, null, 2), canAllow: hasDetails && (!p.availableDecisions || p.availableDecisions.includes('accept')) }, answer => respond(permissions ? { permissions: answer.decision === 'allow' ? p.permissions : {}, scope: 'turn' } : { decision: answer.decision === 'allow' ? 'accept' : !p.availableDecisions || p.availableDecisions.includes('decline') ? 'decline' : 'cancel' }));
        } else {
          // shortcut: arbitrary MCP forms need a schema-aware UI; let the user decline until it exists.
          ask(event.id, { kind: 'approval', title: 'Interaction non prise en charge', detail: JSON.stringify(p, null, 2), canAllow: false }, () => {
            if (event.method === 'mcpServer/elicitation/request') respond({ action: 'decline', content: null });
            else send({ id: event.id, error: { code: -32601, message: 'Interaction non prise en charge par Atelier.' } });
          });
        }
        return;
      }
      if (event.method === 'serverRequest/resolved') { remove(p.requestId); return; }
      const itemKey = JSON.stringify([p.threadId, p.item?.id]);
      if (event.method === 'item/started' && p.item?.type === 'fileChange') items.set(itemKey, p.item);
      if (event.method === 'item/completed') items.delete(itemKey);
      if (threadId && p.threadId && p.threadId !== threadId) return;
      if (event.error && event.id !== undefined) throw new Error(event.error.message || 'Erreur du protocole Codex.');
      if (event.id === 'initialize' && !initialized) {
        initialized = true; send({ method: 'initialized' });
        if (apiKey) rpc('login', 'account/login/start', { type: 'apiKey', apiKey });
        else startThread();
      } else if (event.id === 'login') {
        startThread();
      } else if (event.id === 'thread') {
        threadId = event.result?.thread?.id;
        if (!threadId) throw new Error('Session Codex absente.');
        emit({ type: 'thread.started', thread_id: threadId });
        rpc('turn', 'turn/start', { threadId, input: [{ type: 'text', text: prompt }], ...(effort ? { effort } : {}) });
      } else if (event.method === 'thread/tokenUsage/updated') usage = p.tokenUsage?.total;
      else if (event.method === 'turn/started') emit({ type: 'turn.started' });
      else if (event.method === 'item/agentMessage/delta') emit({ type: 'stream_event', event: { delta: { type: 'text_delta', text: p.delta } } });
      else if (['item/started', 'item/completed'].includes(event.method)) {
        const item = p.item || {};
        const types = { agentMessage: 'agent_message', commandExecution: 'command_execution', fileChange: 'file_change' };
        if (types[item.type]) emit({ type: event.method.replace('/', '.'), item: { ...item, type: types[item.type], aggregated_output: item.aggregatedOutput } });
      } else if (event.method === 'error') {
        if (!p.willRetry) emit({ type: 'error', message: p.error?.message || 'Erreur Codex.' });
      } else if (event.method === 'turn/completed') {
        const success = p.turn?.status === 'completed';
        emit(success ? { type: 'turn.completed', ...(usage ? { usage: { input_tokens: usage.inputTokens, cached_input_tokens: usage.cachedInputTokens, output_tokens: usage.outputTokens } } : {}) } : { type: 'turn.failed', error: p.turn?.error || { message: 'Exécution interrompue.' } });
        complete(success ? 0 : 1);
      }
    },
  };
}
