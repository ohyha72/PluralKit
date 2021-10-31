using System;
using System.Linq;
using System.Threading.Tasks;

using Microsoft.AspNetCore.Mvc;

using Newtonsoft.Json.Linq;

using PluralKit.Core;

namespace PluralKit.API
{
    [ApiController]
    [ApiVersion("2.0")]
    [Route("v{version:apiVersion}")]
    public class GroupControllerV2: PKControllerBase
    {
        public GroupControllerV2(IServiceProvider svc) : base(svc) { }

        [HttpGet("systems/{systemRef}/groups")]
        public async Task<IActionResult> GetSystemGroups(string systemRef)
        {
            var system = await ResolveSystem(systemRef);
            if (system == null)
                throw Errors.SystemNotFound;

            var ctx = this.ContextFor(system);

            if (!system.GroupListPrivacy.CanAccess(User.ContextFor(system)))
                throw Errors.UnauthorizedGroupList;

            var groups = _repo.GetSystemGroups(system.Id);
            return Ok(await groups
                .Where(g => g.Visibility.CanAccess(ctx))
                .Select(g => g.ToJson(ctx))
                .ToListAsync());
        }

        [HttpPost("groups")]
        public async Task<IActionResult> GroupCreate([FromBody] JObject data)
        {
            var system = await ResolveSystem("@me");

            var patch = GroupPatch.FromJson(data);
            patch.AssertIsValid();
            if (!patch.Name.IsPresent)
                patch.Errors.Add(new ValidationError("name", $"Key 'name' is required when creating new group."));
            if (patch.Errors.Count > 0)
                throw new ModelParseError(patch.Errors);

            using var conn = await _db.Obtain();
            using var tx = await conn.BeginTransactionAsync();

            var newGroup = await _repo.CreateGroup(system.Id, patch.Name.Value, conn);
            newGroup = await _repo.UpdateGroup(newGroup.Id, patch, conn);

            await tx.CommitAsync();

            return Ok(newGroup.ToJson(LookupContext.ByOwner));
        }

        [HttpGet("groups/{groupRef}")]
        public async Task<IActionResult> GroupGet(string groupRef)
        {
            var group = await ResolveGroup(groupRef);
            if (group == null)
                throw Errors.GroupNotFound;

            var system = await _repo.GetSystem(group.System);

            return Ok(group.ToJson(this.ContextFor(group), systemStr: system.Hid));
        }

        [HttpPatch("groups/{groupRef}")]
        public async Task<IActionResult> DoGroupPatch(string groupRef, [FromBody] JObject data)
        {
            var system = await ResolveSystem("@me");
            var group = await ResolveGroup(groupRef);
            if (group == null)
                throw Errors.GroupNotFound;
            if (group.System != system.Id)
                throw Errors.NotOwnGroupError;

            var patch = GroupPatch.FromJson(data);

            patch.AssertIsValid();
            if (patch.Errors.Count > 0)
                throw new ModelParseError(patch.Errors);

            var newGroup = await _repo.UpdateGroup(group.Id, patch);
            return Ok(newGroup.ToJson(LookupContext.ByOwner));
        }

        [HttpDelete("groups/{groupRef}")]
        public async Task<IActionResult> GroupDelete(string groupRef)
        {
            var group = await ResolveGroup(groupRef);
            if (group == null)
                throw Errors.GroupNotFound;

            var system = await ResolveSystem("@me");
            if (system.Id != group.System)
                throw Errors.NotOwnGroupError;

            await _repo.DeleteGroup(group.Id);

            return NoContent();
        }
    }
}