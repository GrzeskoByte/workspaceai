// Shows that a chat still has work in progress, wherever chats are listed.
import { useStore, type UiState } from '../store.ts';
import { chatsWithin } from '../../../../packages/shared/src/util.ts';

type Activity = 'working' | 'waiting';

export function activityOf(s: UiState, chatId: string): Activity | null {
  if (s.permissions[chatId]?.length) return 'waiting';
  return s.snap?.chatActivity[chatId] || (s.live[chatId] ? 'working' : null);
}

function Mark({ activity }: { activity: Activity }) {
  const label = activity === 'waiting' ? 'Waiting for your approval' : 'Working';
  return <span className={'chat-activity ' + activity} role="img" aria-label={label} title={label} />;
}

export function ChatActivity({ chatId }: { chatId: string }) {
  const activity = useStore((s) => activityOf(s, chatId));
  return activity && <Mark activity={activity} />;
}

/** For a collapsed project, whose chats are out of sight: waiting wins over working. */
export function ProjectActivity({ projectId }: { projectId: string }) {
  const activity = useStore((s) => {
    if (!s.snap) return null;
    const all = chatsWithin(s.snap.projects, s.snap.chats, projectId).map((c) => activityOf(s, c.id));
    return all.includes('waiting') ? 'waiting' : all.includes('working') ? 'working' : null;
  });
  return activity && <Mark activity={activity} />;
}
