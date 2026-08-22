# Identify production Event Stores independently of Images

Production Mode assigns one stable Store Identity in the Event Schema Lock. Compatible releases carrying that identity open the same host-backed Event Store even though their Image digests differ. Greenfield Mode instead uses project-local development storage and archives it before starting clean after an incompatible schema change.

The initial launcher provides one exclusive fixed-size host-managed block image per Store Identity and host user, validates its geometry and schema lifecycle before boot completes, and never exposes its host path to Creator code. Two live Images cannot bind the same Store Identity. Games model multiple player profiles inside their own Events rather than requesting multiple block volumes. This keeps Image identity, persistent lineage, and host storage location separate.
