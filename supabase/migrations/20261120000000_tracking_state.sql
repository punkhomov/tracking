create table public.tracking_state (
    user_id uuid primary key default auth.uid() references auth.users (id) on delete cascade,
    revision bigint not null check (revision > 0),
    format smallint not null check (format = 1),
    data jsonb not null check (jsonb_typeof(data) = 'object')
);

alter table public.tracking_state enable row level security;
revoke all on public.tracking_state from public, anon, authenticated;
grant usage on schema public to authenticated;
grant select, insert on public.tracking_state to authenticated;
grant update (revision, data) on public.tracking_state to authenticated;

create policy tracking_select_own on public.tracking_state for select to authenticated
    using ((select auth.uid()) = user_id);

create policy tracking_insert_own on public.tracking_state for insert to authenticated
    with check ((select auth.uid()) = user_id);

create policy tracking_update_own on public.tracking_state for update to authenticated
    using ((select auth.uid()) = user_id)
    with check ((select auth.uid()) = user_id);
