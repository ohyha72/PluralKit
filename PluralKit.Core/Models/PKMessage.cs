using Newtonsoft.Json.Linq;

using NodaTime;

namespace PluralKit.Core
{
    public class PKMessage
    {
        public ulong Mid { get; set; }
        public ulong? Guild { get; set; } // null value means "no data" (ie. from before this field being added)
        public ulong Channel { get; set; }
        public MemberId Member { get; set; }
        public ulong Sender { get; set; }
        public ulong? OriginalMid { get; set; }
    }

    public class FullMessage
    {
        public PKMessage Message;
        public PKMember Member;
        public PKSystem System;

        public JObject ToJson(LookupContext ctx, APIVersion v)
        {
            var o = new JObject();

            o.Add("timestamp", Instant.FromUnixTimeMilliseconds((long)(this.Message.Mid >> 22) + 1420070400000).ToString());
            o.Add("id", this.Message.Mid.ToString());
            o.Add("original", this.Message.OriginalMid.ToString());
            o.Add("sender", this.Message.Sender.ToString());
            o.Add("channel", this.Message.Channel.ToString());
            o.Add("guild", this.Message.Guild?.ToString());
            o.Add("system", this.System.ToJson(ctx, v));
            o.Add("member", this.Member.ToJson(ctx, v: v));

            return o;
        }
    }
}