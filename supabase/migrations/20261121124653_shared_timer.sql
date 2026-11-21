create table public.tracking_timer (
    user_id uuid primary key default auth.uid() references auth.users (id) on delete cascade,
    revision bigint not null default 0 check (revision >= 0),
    change_id uuid,
    timer jsonb check (timer is null or jsonb_typeof(timer) = 'object'),
    next_kind text not null default 'work' check (next_kind in ('work', 'break'))
);

alter table public.tracking_timer enable row level security;
revoke all on public.tracking_timer from public, anon, authenticated;
grant select, insert on public.tracking_timer to authenticated;
grant update (revision, change_id, timer, next_kind) on public.tracking_timer to authenticated;

create policy tracking_timer_select_own on public.tracking_timer
    for select to authenticated using ((select auth.uid()) = user_id);
create policy tracking_timer_insert_own on public.tracking_timer
    for insert to authenticated with check ((select auth.uid()) = user_id);
create policy tracking_timer_update_own on public.tracking_timer
    for update to authenticated using ((select auth.uid()) = user_id)
    with check ((select auth.uid()) = user_id);

alter publication supabase_realtime add table public.tracking_timer;

create function public.tracking_apply_timer(
    expected_revision bigint,
    operation_id uuid,
    next_timer jsonb,
    selected_kind text,
    finished_id uuid,
    finished_period jsonb
)
returns table (revision bigint, change_id uuid, timer jsonb, next_kind text)
language plpgsql
security invoker
set search_path = ''
as $$
declare
    owner_id uuid := (select auth.uid());
begin
    if owner_id is null or operation_id is null or expected_revision is null then
        raise exception 'Missing timer operation identity';
    end if;
    if (finished_id is null) <> (finished_period is null)
        or (finished_period is not null and jsonb_typeof(finished_period) <> 'object') then
        raise exception 'Invalid finished period';
    end if;

    insert into public.tracking_timer (user_id) values (owner_id)
    on conflict (user_id) do nothing;

    update public.tracking_timer as t
    set revision = t.revision + 1,
        change_id = operation_id,
        timer = next_timer,
        next_kind = selected_kind
    where t.user_id = owner_id
      and t.revision = expected_revision
      and t.change_id is distinct from operation_id
      and (finished_id is null or t.timer ->> 'id' = finished_id::text);

    if found and finished_id is not null then
        insert into public.tracking_state as s (user_id, revision, format, data)
        values (
            owner_id, 1, 1,
            jsonb_build_object(
                'periods', jsonb_build_object(finished_id::text, finished_period),
                'categories', '{}'::jsonb, 'zones', '{}'::jsonb, 'settings', '{}'::jsonb
            )
        )
        on conflict (user_id) do update
        set revision = s.revision + 1,
            data = jsonb_set(s.data, array['periods', finished_id::text], finished_period);
    end if;

    return query
    select t.revision, t.change_id, t.timer, t.next_kind
    from public.tracking_timer as t where t.user_id = owner_id;
end;
$$;

revoke all on function public.tracking_apply_timer(bigint, uuid, jsonb, text, uuid, jsonb)
    from public, anon, authenticated;
grant execute on function public.tracking_apply_timer(bigint, uuid, jsonb, text, uuid, jsonb)
    to authenticated;
